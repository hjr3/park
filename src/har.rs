use har::v1_3::{Cache, Content, Creator, Entries, Log, PostData, Request, Response, Timings};
use http_body_util::BodyExt;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Har(Log);

impl Har {
    pub async fn from_transaction<T: BodyExt, U: BodyExt>(
        req: hyper::Request<T>,
        resp: hyper::Response<U>,
    ) -> Self {
        let (req, req_body) = req.into_parts();
        let req_text = body_to_string(req_body).await;

        let (res, res_body) = resp.into_parts();
        let res_text = body_to_string(res_body).await;

        let entry = Entries {
            pageref: None,
            started_date_time: "".to_string(),
            time: 0.0,
            request: Request {
                method: req.method.as_str().to_string(),
                url: req.uri.to_string(),
                http_version: display_version(req.version),
                cookies: vec![],
                headers: vec![],
                query_string: vec![],
                post_data: Some(PostData {
                    mime_type: req
                        .headers
                        .get("content-type")
                        .map(|v| v.to_str().unwrap_or("application/octet-stream"))
                        .unwrap_or("application/octet-stream")
                        .to_string(),
                    text: req_text,
                    params: None,
                    comment: None,
                    encoding: None,
                }),
                headers_size: 0,
                body_size: 0,
                comment: None,
                headers_compression: None,
            },
            response: Response {
                status: res.status.as_u16() as i64,
                status_text: res
                    .status
                    .canonical_reason()
                    .unwrap_or_default()
                    .to_string(),
                http_version: display_version(res.version),
                cookies: vec![],
                headers: vec![],
                content: Content {
                    size: 0,
                    compression: None,
                    mime_type: Some(
                        res.headers
                            .get("content-type")
                            .map(|v| v.to_str().unwrap_or("application/octet-stream"))
                            .unwrap_or("application/octet-stream")
                            .to_string(),
                    ),
                    text: res_text,
                    encoding: None,
                    comment: None,
                },
                redirect_url: None,
                headers_size: 0,
                body_size: 0,
                comment: None,
                headers_compression: None,
            },
            cache: Cache {
                before_request: None,
                after_request: None,
            },
            timings: Timings {
                blocked: None,
                dns: None,
                connect: None,
                send: 0.0,
                wait: 0.0,
                receive: 0.0,
                ssl: None,
                comment: None,
            },
            server_ip_address: None,
            connection: None,
            comment: None,
        };

        let log = Log {
            creator: Creator {
                name: "park".to_string(),
                version: "0.1.0".to_string(),
                comment: None,
            },
            browser: None,
            pages: None,
            entries: vec![entry],
            comment: None,
        };

        Har(log)
    }
}

fn display_version(v: http::Version) -> String {
    format!("{:?}", v)
}

async fn body_to_string<T: BodyExt>(body: T) -> Option<String> {
    body.collect()
        .await
        .map(|b| {
            let bytes = b.to_bytes();
            String::from_utf8(bytes.to_vec())
                .map_err(|e| {
                    tracing::error!("Error converting request body to string: {:?}", e);
                    e
                })
                .unwrap_or_default()
        })
        .map_err(|e| {
            tracing::error!("Error collecting request body");
            e
        })
        .ok()
}
