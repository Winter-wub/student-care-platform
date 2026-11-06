//! [`ReqwestHttp`]: the `HttpClient` port over reqwest (rustls, HTTP/2).

use std::time::Duration;

use async_trait::async_trait;
use harpseal::{Error, HttpClient, Result};

/// Outgoing HTTP with one pooled reqwest client. 30 s timeout per request.
#[derive(Debug, Clone)]
pub struct ReqwestHttp {
    client: reqwest::Client,
}

fn platform(e: impl core::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

impl ReqwestHttp {
    /// A client with the default timeout.
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("harpseal/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(platform)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestHttp {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        let req = reqwest::Request::try_from(req.map(reqwest::Body::from)).map_err(platform)?;
        let resp = self.client.execute(req).await.map_err(platform)?;
        let mut out = http::Response::builder().status(resp.status());
        if let Some(h) = out.headers_mut() {
            h.extend(resp.headers().clone());
        }
        let body = resp.bytes().await.map_err(platform)?;
        out.body(body.to_vec()).map_err(platform)
    }
}
