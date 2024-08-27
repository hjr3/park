use anyhow::Result;
use http_body_util::BodyStream;
use http_body_util::StreamBody;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::body::{Bytes, Frame};
use hyper::client::conn::http1::Builder;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use std::sync::Arc;
use tokio::net::TcpStream;

use futures_util::stream::StreamExt;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

pub mod config;
mod db;
mod har;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
}

pub async fn proxy(
    config: Arc<config::Config>,
    state: AppState,
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, anyhow::Error>>, anyhow::Error> {
    tracing::info!("{:?}", req);

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
        let ip = config.server.addr.ip();
        let port = config.server.addr.port();

        let stream = TcpStream::connect((ip, port)).await.unwrap();
        let io = TokioIo::new(stream);

        let (mut sender, conn) = Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .handshake(io)
            .await?;

        tokio::task::spawn(async move {
            if let Err(err) = conn.await {
                println!("Connection failed: {:?}", err);
            }
        });

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

        let upstream_stream = BroadcastStream::new(upstream_rx).map(|b| match b {
            Ok(bytes) => Ok(Frame::data(bytes)),
            Err(e) => Err(e),
        });
        let upstream_body = StreamBody::new(upstream_stream);
        let upstream_req = Request::from_parts(head.clone(), upstream_body);

        let resp = sender.send_request(upstream_req).await?;

        let (response_head, resp_body) = resp.into_parts();
        let (resp_tx, downstream_rx) = broadcast::channel(16);
        let har_response_rx = resp_tx.subscribe();

        tokio::spawn(async move {
            let mut body_stream = BodyStream::new(resp_body);

            while let Some(chunk_result) = body_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        // FIXME: handle all frame types
                        let bytes = chunk.into_data().unwrap_or_default();
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
        let downstream_resp = Response::from_parts(response_head.clone(), downstream_body);

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
            let har_resp = Response::from_parts(response_head, har_body);

            let har = har::Har::from_transaction(har_req, har_resp).await;
            let _ = db::insert_request(&state.db, &har).await.map_err(|e| {
                tracing::error!("Error while saving HAR: {}", e);
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
