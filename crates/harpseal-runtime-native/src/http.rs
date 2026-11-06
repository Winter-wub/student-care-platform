//! [`ReqwestHttp`]: the `HttpClient` port over reqwest (rustls, HTTP/2).
//!
//! Errors never carry the request URL: reqwest quotes it in its errors
//! ("error sending request for url (…)"), and adapters put secrets in URLs
//! (Telegram's `…/bot<token>/…`). Every reqwest error is stripped with
//! [`reqwest::Error::without_url`] before it becomes an [`Error`], so no
//! adapter has to redact tokens itself (they may still, as defence in
//! depth).

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

/// A reqwest error without the URL it may quote.
fn transport(e: reqwest::Error) -> Error {
    platform(e.without_url())
}

impl ReqwestHttp {
    /// A client with the default timeout.
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("harpseal/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(transport)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestHttp {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        let req = reqwest::Request::try_from(req.map(reqwest::Body::from)).map_err(transport)?;
        let resp = self.client.execute(req).await.map_err(transport)?;
        let mut out = http::Response::builder().status(resp.status());
        if let Some(h) = out.headers_mut() {
            h.extend(resp.headers().clone());
        }
        let body = resp.bytes().await.map_err(transport)?;
        out.body(body.to_vec()).map_err(platform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_transport_error_never_quotes_the_url_or_its_token() {
        // Nothing listens on port 1: the request fails before any answer.
        let token = "7700000001:SEALBAY-secret-bot-token";
        let url = format!("http://127.0.0.1:1/bot{token}/sendMessage?chat_id=42");
        let req = http::Request::post(&url).body(Vec::new()).unwrap();
        let http = ReqwestHttp::new().unwrap();
        let err = http.send(req).await.expect_err("port 1 refuses");
        let text = err.to_string();
        assert!(matches!(err, Error::Platform(_)), "{err:?}");
        assert!(!text.contains(token), "token leaked: {text}");
        assert!(!text.contains("127.0.0.1:1"), "URL leaked: {text}");
        assert!(!text.contains("sendMessage"), "URL leaked: {text}");
        assert!(!format!("{err:?}").contains(token), "token leaked in Debug");
    }
}
