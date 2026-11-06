//! [`ResendEmail`]: the `EmailSender` both runtimes share. It calls the
//! Resend HTTP API through the [`HttpClient`] port, so the same code runs on
//! Workers (`fetch`) and natively (reqwest), and tests drive it with a
//! recording client.

use std::sync::Arc;

use async_trait::async_trait;

use crate::port_types::Email;
use crate::ports::{EmailSender, HttpClient};
use crate::{Error, Result};

/// Resend's send endpoint (<https://resend.com/docs/api-reference/emails/send-email>).
pub const RESEND_ENDPOINT: &str = "https://api.resend.com/emails";

/// Sends email with the Resend HTTP API (`POST /emails`, bearer API key).
/// Any non-2xx answer is an [`Error::Platform`] carrying Resend's body.
pub struct ResendEmail {
    http: Arc<dyn HttpClient>,
    api_key: String,
    from: String,
    endpoint: String,
}

impl ResendEmail {
    /// A sender using `api_key` (a secret, never from TOML) and the verified
    /// sender address `from` (`"Name <addr@domain>"` or a bare address).
    pub fn new(
        http: Arc<dyn HttpClient>,
        api_key: impl Into<String>,
        from: impl Into<String>,
    ) -> Self {
        Self {
            http,
            api_key: api_key.into(),
            from: from.into(),
            endpoint: RESEND_ENDPOINT.into(),
        }
    }

    /// Sends to another endpoint (a mock server in tests).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    fn body(&self, email: &Email) -> String {
        let mut b = String::with_capacity(email.text.len() + 128);
        b.push_str("{\"from\":");
        json_str(&mut b, &self.from);
        b.push_str(",\"to\":[");
        json_str(&mut b, &email.to);
        b.push_str("],\"subject\":");
        json_str(&mut b, &email.subject);
        b.push_str(",\"text\":");
        json_str(&mut b, &email.text);
        if let Some(html) = &email.html {
            b.push_str(",\"html\":");
            json_str(&mut b, html);
        }
        b.push('}');
        b
    }
}

/// Appends `s` as a JSON string literal (RFC 8259 escaping).
fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[async_trait]
impl EmailSender for ResendEmail {
    async fn send(&self, email: Email) -> Result<()> {
        let req = http::Request::post(&self.endpoint)
            .header(
                http::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(self.body(&email).into_bytes())
            .map_err(|e| Error::Platform(e.to_string()))?;
        let resp = self.http.send(req).await?;
        if resp.status().is_success() {
            return Ok(());
        }
        let body = String::from_utf8_lossy(resp.body());
        Err(Error::Platform(format!(
            "resend answered {}: {}",
            resp.status().as_u16(),
            body.chars().take(300).collect::<String>()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::json_str;

    #[test]
    fn escapes_json() {
        let mut s = String::new();
        json_str(&mut s, "a\"b\\c\nd\u{1}é");
        assert_eq!(s, "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }
}
