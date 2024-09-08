use anyhow::Result;
use http_body_util::BodyStream;
use http_body_util::StreamBody;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::{Body, Bytes, Frame};
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use std::sync::Arc;
use tokio::net::TcpStream;

use futures_util::stream::StreamExt;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::config;
use crate::har;
use crate::AppState;

pub async fn proxy<B>(
    config: Arc<config::Config>,
    state: AppState,
    req: Request<B>,
) -> Result<Response<BoxBody<Bytes, anyhow::Error>>, anyhow::Error>
where
    B: Body + std::fmt::Debug + std::marker::Unpin + Send + 'static,
    B::Data: Clone + Default + Send + 'static,
    B::Error: Into<anyhow::Error> + std::fmt::Display,
    hyper::body::Bytes: From<<B as hyper::body::Body>::Data>,
{
    tracing::trace!("{:?}", req);

    if Method::CONNECT == req.method() {
        // Received an HTTP request like:
        // ```
        // CONNECT www.domain.com:443 HTTP/1.1
        // Host: www.domain.com:443
        // Proxy-Connection: Keep-Alive
        // ```
        //
        // When HTTP method is CONNECT we should return an empty body
        // then we can eventually upgrade the connection and talk a new protocol.
        //
        // Note: only after client received an empty body with STATUS_OK can the
        // connection be upgraded, so we can't return a response inside
        // `on_upgrade` future.
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr).await {
                            eprintln!("server io error: {}", e);
                        };
                    }
                    Err(e) => eprintln!("upgrade error: {}", e),
                }
            });

            Ok(Response::new(empty()))
        } else {
            eprintln!("CONNECT host is not socket addr: {:?}", req.uri());
            let mut resp = Response::new(full("CONNECT must be to a socket address"));
            *resp.status_mut() = http::StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else {
        let mut upstream_url = config.server.address.clone();

        let (head, body) = req.into_parts();
        let (tx, upstream_rx) = broadcast::channel(16);
        let har_rx = tx.subscribe();

        tokio::spawn(async move {
            let mut body_stream = BodyStream::new(body);

            while let Some(chunk_result) = body_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        // FIXME: handle all frame types
                        let bytes = chunk.into_data().unwrap_or_default();
                        if tx.send(bytes).is_err() {
                            // All receivers have dropped
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error while reading body: {}", e);
                        break;
                    }
                }
            }
        });

        let upstream_stream = BroadcastStream::new(upstream_rx);

        upstream_url.set_path(head.uri.path());

        let upstream_req = state
            .client
            .request(From::from(&head.method), upstream_url)
            .version(head.version)
            .headers(head.headers.clone())
            .body(reqwest::Body::wrap_stream(upstream_stream))
            .build()?;

        let resp = state.client.execute(upstream_req).await?;

        let resp_status = resp.status();
        let resp_version = resp.version();
        let resp_extension = resp.extensions().clone();
        let resp_headers = resp.headers().clone();
        let mut resp_body = resp.bytes_stream();

        let (resp_tx, downstream_rx) = broadcast::channel(16);
        let har_response_rx = resp_tx.subscribe();

        tokio::spawn(async move {
            while let Some(chunk_result) = resp_body.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        if resp_tx.send(bytes).is_err() {
                            // All receivers have dropped
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error while reading body: {}", e);
                        break;
                    }
                }
            }
        });

        let downstream_stream = BroadcastStream::new(downstream_rx).map(|b| match b {
            Ok(bytes) => Ok(Frame::data(bytes)),
            Err(e) => Err(e.into()),
        });
        let downstream_body = StreamBody::new(downstream_stream);
        let mut downstream_resp = Response::builder()
            .status(resp_status)
            .version(resp_version)
            .extension(resp_extension.clone());

        for (key, value) in resp_headers.iter() {
            downstream_resp = downstream_resp.header(key, value);
        }

        let downstream_resp = downstream_resp.body(downstream_body)?;

        tokio::spawn(async move {
            let har_stream = BroadcastStream::new(har_rx).map(|b| match b {
                Ok(bytes) => Ok(Frame::data(bytes)),
                Err(e) => Err(e),
            });
            let har_body = StreamBody::new(har_stream);
            let har_req = Request::from_parts(head, har_body);

            let har_stream = BroadcastStream::new(har_response_rx).map(|b| match b {
                Ok(bytes) => Ok(Frame::data(bytes)),
                Err(e) => Err(e),
            });
            let har_body = StreamBody::new(har_stream);
            let mut har_resp = Response::builder()
                .status(resp_status)
                .version(resp_version)
                .extension(resp_extension);

            for (key, value) in resp_headers.iter() {
                har_resp = har_resp.header(key, value);
            }

            let har_resp = match har_resp.body(har_body) {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::error!("Error while creating HAR response: {}", e);
                    return;
                }
            };

            let har = har::Har::from_transaction(har_req, har_resp).await;
            let _ = state.har_queue.send(har).await.map_err(|e| {
                tracing::error!("Error while queueing HAR: {}", e);
            });
        });

        Ok(downstream_resp.map(http_body_util::BodyExt::boxed))
    }
}

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}

fn empty() -> BoxBody<Bytes, anyhow::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, anyhow::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
async fn tunnel(upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;
    let mut upgraded = TokioIo::new(upgraded);

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

    // Print message when done
    println!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );

    Ok(())
}
