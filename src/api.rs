use anyhow::Result;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use std::sync::Arc;

use crate::config;
use crate::har::Har;
use crate::proxy::proxy;
use crate::AppState;

pub async fn api(
    config: Arc<config::Config>,
    state: AppState,
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, anyhow::Error>>, anyhow::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => {
            let body = Full::new(Bytes::from_static(b"Hello, World!")).map_err(anyhow::Error::from);
            Ok(Response::new(BoxBody::new(body)))
        }
        (&Method::POST, "/requests") => proxy_request(config, state, req).await,
        _ => {
            let body = Full::new(Bytes::from_static(b"Not found")).map_err(anyhow::Error::from);
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(BoxBody::new(body))
                .unwrap())
        }
    }
}

async fn proxy_request(
    config: Arc<config::Config>,
    state: AppState,
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, anyhow::Error>>, anyhow::Error> {
    let body = req.collect().await?.to_bytes();
    let har: Har = serde_json::from_slice(&body)?;

    let req = hyper::Request::try_from(har)?;

    let res = proxy(config, state, req).await?;

    Ok(res)
}
