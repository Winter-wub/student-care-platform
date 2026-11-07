//! HTTP calls to the Messaging API through the `HttpClient` port, and how
//! LINE's error statuses become Core errors
//! (<https://developers.line.biz/en/reference/messaging-api/#status-codes>).

use harpseal::{Error, Result};
use http::{Method, header};
use serde_json::Value;

use crate::LineChannel;

/// Messaging API host.
pub const API: &str = "https://api.line.me";
/// Host for binary content (message content, rich menu images).
pub const API_DATA: &str = "https://api-data.line.me";

/// Header carrying a request's idempotency key on push and multicast
/// (<https://developers.line.biz/en/reference/messaging-api/#retry-api-request>).
pub const RETRY_KEY_HEADER: &str = "x-line-retry-key";

/// Seconds to wait after a 429 without `Retry-After` (LINE does not
/// document one; the rate limits are per minute or per second:
/// <https://developers.line.biz/en/reference/messaging-api/#rate-limits>).
pub const DEFAULT_RETRY_AFTER_SECS: u32 = 60;

/// A non-2xx answer, before it becomes an [`Error`].
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) status: u16,
    pub(crate) message: String,
    retry_after: Option<u32>,
}

impl Failure {
    /// The Core error. `to_user`: the call sent messages to one account,
    /// so a 403 means the account cannot be reached (blocked the bot or
    /// never added it) and becomes `Error::Unreachable`.
    pub(crate) fn into_error(self, to_user: bool) -> Error {
        match self.status {
            429 if self.monthly_limit() => Error::Platform(format!(
                "LINE monthly message quota used up (429): {}",
                self.message
            )),
            429 => Error::RateLimited {
                retry_after_secs: self.retry_after.unwrap_or(DEFAULT_RETRY_AFTER_SECS),
            },
            403 if to_user => Error::Unreachable,
            401 => Error::Platform("LINE refused the channel access token (401)".into()),
            s => Error::Platform(format!("LINE answered {s}: {}", self.message)),
        }
    }

    /// LINE answers 429 both for rate limits and for a used-up monthly
    /// message quota ("You have reached your monthly limit."); waiting
    /// seconds does not help the latter
    /// (<https://developers.line.biz/en/reference/messaging-api/#status-codes>).
    fn monthly_limit(&self) -> bool {
        self.message.to_ascii_lowercase().contains("monthly limit")
    }

    /// LINE refused the reply token (expired or already used).
    pub(crate) fn bad_reply_token(&self) -> bool {
        self.status == 400 && self.message.to_ascii_lowercase().contains("reply token")
    }
}

/// What a call returned.
pub(crate) type Answer = std::result::Result<Vec<u8>, Failure>;

/// LINE ids (user `U` + 32 hex, message ids, `richmenu-<hex>`) as URL path
/// segments: anything outside `[A-Za-z0-9-]` is refused.
pub(crate) fn path_id(id: &str) -> Result<&str> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        Ok(id)
    } else {
        Err(Error::Invalid(format!("not a LINE id: {id:?}")))
    }
}

impl LineChannel {
    /// One authorised call. `Err` for transport problems, `Ok(Err)` for a
    /// non-2xx status.
    pub(crate) async fn call(
        &self,
        method: Method,
        url: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> Result<Answer> {
        let resp = self
            .request(method, url, content_type, body, true, None)
            .await?;
        Ok(answer(resp))
    }

    /// One call, with the channel access token only when `authorised`
    /// (never to LINE Login endpoints). The token header is marked
    /// sensitive so HTTP clients do not log it.
    pub(crate) async fn send_raw(
        &self,
        method: Method,
        url: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        authorised: bool,
    ) -> Result<http::Response<Vec<u8>>> {
        self.request(method, url, content_type, body, authorised, None)
            .await
    }

    /// [`LineChannel::send_raw`], with header [`RETRY_KEY_HEADER`] when
    /// `retry_key` is given.
    async fn request(
        &self,
        method: Method,
        url: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        authorised: bool,
        retry_key: Option<&str>,
    ) -> Result<http::Response<Vec<u8>>> {
        let mut req = http::Request::builder().method(method).uri(url);
        if let Some(k) = retry_key {
            req = req.header(RETRY_KEY_HEADER, k);
        }
        if authorised {
            let mut v = header::HeaderValue::from_str(&format!("Bearer {}", self.cfg.access_token))
                .map_err(|_| {
                    Error::Invalid("the LINE access token is not a header value".into())
                })?;
            v.set_sensitive(true);
            req = req.header(header::AUTHORIZATION, v);
        }
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let req = req
            .body(body)
            .map_err(|e| Error::Platform(format!("LINE request: {e}")))?;
        self.http.send(req).await
    }

    /// `POST url` with a JSON body; the answer parsed (`{}` when empty).
    pub(crate) async fn post_json(
        &self,
        url: &str,
        body: &Value,
    ) -> Result<std::result::Result<Value, Failure>> {
        let got = self.post_json_keyed(url, body, None).await?;
        Ok(got.map(|a| a.body))
    }

    /// [`LineChannel::post_json`] with `X-Line-Retry-Key: retry_key`. LINE
    /// answers 409 when a request with that key was already accepted (it
    /// went out earlier): that counts as sent, with no message ids and
    /// [`Keyed::replayed`] set
    /// (<https://developers.line.biz/en/docs/messaging-api/retrying-api-request/>).
    pub(crate) async fn post_json_keyed(
        &self,
        url: &str,
        body: &Value,
        retry_key: Option<&str>,
    ) -> Result<std::result::Result<Keyed, Failure>> {
        let bytes = serde_json::to_vec(body).map_err(|e| Error::Platform(e.to_string()))?;
        let resp = self
            .request(
                Method::POST,
                url,
                Some("application/json"),
                bytes,
                true,
                retry_key,
            )
            .await?;
        Ok(match answer(resp) {
            Err(f) if retry_key.is_some() && f.status == 409 => Ok(Keyed {
                body: Value::Null,
                replayed: true,
            }),
            got => got.map(|b| Keyed {
                body: parse(&b),
                replayed: false,
            }),
        })
    }
}

/// A keyed request LINE accepted ([`LineChannel::post_json_keyed`]).
#[derive(Debug)]
pub(crate) struct Keyed {
    /// The answer parsed (`Null` for a 409).
    pub(crate) body: Value,
    /// LINE answered 409: a request with this retry key was accepted on
    /// an earlier attempt, and nothing went out now.
    pub(crate) replayed: bool,
}

/// The body of a 2xx, or the failure with LINE's `message`.
pub(crate) fn answer(resp: http::Response<Vec<u8>>) -> Answer {
    let status = resp.status().as_u16();
    if resp.status().is_success() {
        return Ok(resp.into_body());
    }
    let retry_after = resp
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let v = parse(resp.body());
    let message = v["message"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            String::from_utf8_lossy(resp.body())
                .chars()
                .take(200)
                .collect()
        });
    Err(Failure {
        status,
        message,
        retry_after,
    })
}

/// JSON, or `Value::Null` for an empty or non-JSON body.
pub(crate) fn parse(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail(status: u16, retry: Option<&str>, body: &str) -> Failure {
        let mut b = http::Response::builder().status(status);
        if let Some(r) = retry {
            b = b.header(header::RETRY_AFTER, r);
        }
        answer(b.body(body.as_bytes().to_vec()).unwrap()).unwrap_err()
    }

    #[test]
    fn statuses_map_to_core_errors() {
        let e = fail(429, Some("7"), "{}").into_error(true);
        assert!(matches!(
            e,
            Error::RateLimited {
                retry_after_secs: 7
            }
        ));
        let e = fail(429, None, "{}").into_error(false);
        assert!(matches!(
            e,
            Error::RateLimited {
                retry_after_secs: 60
            }
        ));
        assert!(matches!(
            fail(403, None, "{}").into_error(true),
            Error::Unreachable
        ));
        assert!(matches!(
            fail(403, None, "{}").into_error(false),
            Error::Platform(_)
        ));
        let e = fail(
            400,
            None,
            r#"{"message":"The request body has 1 error(s)"}"#,
        );
        assert!(e.into_error(true).to_string().contains("1 error(s)"));
        assert!(fail(400, None, r#"{"message":"Invalid reply token"}"#).bad_reply_token());
    }

    #[test]
    fn ids_in_paths_are_alphanumeric() {
        assert!(path_id("U5ea1ba7000000000000000000000c0de").is_ok());
        assert!(path_id("richmenu-8dfdfc571eca39c0ffcd1f799519c5b5").is_ok());
        assert!(path_id("U1/../all").is_err());
        assert!(path_id("").is_err());
    }
}
