//! Mini App = LIFF. Launch URLs are `https://liff.line.me/{liffId}{path}`
//! (<https://developers.line.biz/en/docs/liff/opening-liff-app/>), or
//! `https://miniapp.line.me/{liffId}{path}` for a LINE MINI App
//! (<https://developers.line.biz/en/docs/line-mini-app/develop/permanent-links/>),
//! which is a LIFF app too ([`crate::LineConfig::with_launch_base`]). A page
//! sends the LIFF ID token (`liff.getIDToken()`); it is checked with
//! `POST /oauth2/v2.1/verify`
//! (<https://developers.line.biz/en/reference/line-login/#verify-id-token>)
//! against the LINE Login channel that owns the LIFF app, whose channel id
//! is [`crate::LineConfig::login_channel_id`] (by default the part of the
//! LIFF id before `-`).

use async_trait::async_trait;
use harpseal::{ChannelAccountId, Error, MiniApp, Result, WebClient};
use http::Method;

use crate::api::{API, parse};
use crate::{KIND, LineChannel};

/// Issuer of LINE ID tokens.
pub const ISSUER: &str = "https://access.line.me";
/// Launch URLs of a LIFF app.
pub const LIFF_BASE: &str = "https://liff.line.me/";
/// Launch URLs of a LINE MINI App.
pub const LINE_MINI_APP_BASE: &str = "https://miniapp.line.me/";

impl LineChannel {
    /// `path` is appended to the LIFF URL; one that starts with neither
    /// `/` nor `?` gets a `/` first.
    pub(crate) fn liff_url(&self, path: &str) -> String {
        let sep = if path.is_empty() || path.starts_with(['/', '?']) {
            ""
        } else {
            "/"
        };
        // With or without its trailing slash.
        let base = match self.cfg.launch_base.trim_end_matches('/') {
            "" => LIFF_BASE.trim_end_matches('/'),
            b => b,
        };
        format!("{base}/{}{sep}{path}", self.cfg.liff_id)
    }

    /// The LINE Login channel id of the LIFF app.
    fn login_channel_id(&self) -> &str {
        if self.cfg.login_channel_id.is_empty() {
            self.cfg.liff_id.split('-').next().unwrap_or_default()
        } else {
            &self.cfg.login_channel_id
        }
    }
}

/// `application/x-www-form-urlencoded` value (RFC 3986 unreserved kept).
fn form(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[async_trait]
impl MiniApp for LineChannel {
    fn launch_url(&self, path: &str) -> String {
        self.liff_url(path)
    }

    /// The LIFF SDK with the configured LIFF id, whatever the launch URL.
    fn web_client(&self) -> Option<WebClient> {
        Some(WebClient::Liff {
            id: self.cfg.liff_id.clone(),
        })
    }

    /// The LINE user id of a valid ID token issued for this LIFF app. A
    /// token LINE refuses (expired, wrong channel, forged) or one whose
    /// `iss` / `aud` do not match is `BadSignature`.
    async fn verify_session(&self, id_token: &str) -> Result<ChannelAccountId> {
        let client = self.login_channel_id();
        if id_token.is_empty() || client.is_empty() {
            return Err(Error::BadSignature);
        }
        let body = format!("id_token={}&client_id={}", form(id_token), form(client));
        let url = format!("{API}/oauth2/v2.1/verify");
        let ct = Some("application/x-www-form-urlencoded");
        // LINE Login endpoint: no Messaging API token.
        let resp = self
            .send_raw(Method::POST, &url, ct, body.into_bytes(), false)
            .await?;
        let claims = match crate::api::answer(resp) {
            Ok(b) => parse(&b),
            Err(f) if f.status == 400 || f.status == 401 => return Err(Error::BadSignature),
            Err(f) => return Err(f.into_error(false)),
        };
        let ok = claims["iss"] == ISSUER && claims["aud"] == client;
        match claims["sub"].as_str() {
            Some(sub) if ok && !sub.is_empty() => Ok(ChannelAccountId::new(KIND, sub)),
            _ => Err(Error::BadSignature),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::form;

    #[test]
    fn form_encoding_keeps_jwt_characters() {
        assert_eq!(form("eyJ.a-b_c~"), "eyJ.a-b_c~");
        assert_eq!(form("a+b=/"), "a%2Bb%3D%2F");
    }
}
