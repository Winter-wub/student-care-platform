//! Bot API calls over the Deployment's [`HttpClient`]: one JSON `POST` per
//! method (<https://core.telegram.org/bots/api#making-requests>), the
//! answer envelope `{ ok, result, error_code, description, parameters }`
//! mapped to the Core's [`Error`], and file download via `getFile`.

use harpseal::{Capability, Error, HttpClient, IncomingFile, Result};
use serde_json::Value;

/// Bot API host.
pub const API_BASE: &str = "https://api.telegram.org";
/// Bots download files of at most 20 MB
/// (<https://core.telegram.org/bots/api#getfile>).
pub const DOWNLOAD_MAX: u64 = 20 * 1_048_576;

/// Telegram's error for a message older than 48 hours, which bots cannot
/// delete (<https://core.telegram.org/bots/api#deletemessage>).
const CANNOT_DELETE: &str = "message can't be deleted";

/// Answers that mean the goal is already reached: an edit to the same
/// content (<https://core.telegram.org/bots/api#editmessagetext>) or a
/// delete of a message already gone.
fn already_done(method: &str, body: &Value) -> bool {
    let description = body["description"].as_str().unwrap_or_default();
    match method {
        "editMessageText" => description.contains("message is not modified"),
        "deleteMessage" => description.contains("message to delete not found"),
        _ => false,
    }
}

/// Removes the bot token from an error's text. Port errors (e.g. reqwest's)
/// quote the request URL, which holds the token, and the Kernel logs error
/// texts to `security_event`.
pub(crate) fn redact(e: Error, token: &str) -> Error {
    if token.is_empty() {
        return e;
    }
    let clean = |m: String| m.replace(token, "<bot-token>");
    match e {
        Error::Platform(m) => Error::Platform(clean(m)),
        Error::Invalid(m) => Error::Invalid(clean(m)),
        Error::Db(m) => Error::Db(clean(m)),
        other => other,
    }
}

/// The Bot API's description for a chat that does not exist (any more).
const CHAT_NOT_FOUND: &str = "Bad Request: chat not found";

/// What went wrong in one Bot API call, before it becomes an [`Error`].
fn error_for(method: &str, status: u16, body: &Value) -> Error {
    let code = body["error_code"]
        .as_u64()
        .map(|c| c as u16)
        .unwrap_or(status);
    let description = body["description"].as_str().unwrap_or("no description");
    match code {
        // The Student blocked the bot, or deleted their account
        // (<https://core.telegram.org/bots/faq#broadcasting-to-users>).
        403 => Error::Unreachable,
        // Flood control: `parameters.retry_after` seconds
        // (<https://core.telegram.org/bots/api#responseparameters>).
        429 => Error::RateLimited {
            retry_after_secs: body["parameters"]["retry_after"]
                .as_u64()
                .map(|s| s.min(u64::from(u32::MAX)) as u32)
                .unwrap_or(1),
        },
        // The chat does not exist for this bot (the account was deleted,
        // or the id never started the bot): the Bot API answers
        // `400 Bad Request: chat not found`. The text is not in a
        // documented list, so only that exact phrase counts; a chat id the
        // Kernel stored came from an Update, so it is not a typo.
        400 if description.eq_ignore_ascii_case(CHAT_NOT_FOUND) => Error::Unreachable,
        400 if method == "deleteMessage" && description.contains(CANNOT_DELETE) => {
            Error::Unsupported(Capability::DeleteMessage)
        }
        _ => Error::Platform(format!("Telegram {method}: {code} {description}")),
    }
}

/// A Bot API client for one bot token.
pub(crate) struct Api<'a> {
    pub(crate) http: &'a dyn HttpClient,
    pub(crate) token: &'a str,
}

impl Api<'_> {
    /// Calls `method` with `body` as JSON. `Ok(result)` on success; a 2xx
    /// answer without `ok: false` counts as success (`result` is then
    /// `Null` when absent).
    pub(crate) async fn call(&self, method: &str, body: &Value) -> Result<Value> {
        self.call_raw(method, body)
            .await
            .map_err(|e| redact(e, self.token))
    }

    async fn call_raw(&self, method: &str, body: &Value) -> Result<Value> {
        let req = http::Request::post(format!("{API_BASE}/bot{}/{method}", self.token))
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(body.to_string().into_bytes())
            .map_err(|e| Error::Platform(format!("Telegram {method}: {e}")))?;
        let resp = self.http.send(req).await?;
        let status = resp.status().as_u16();
        let answer: Value = serde_json::from_slice(resp.body()).unwrap_or(Value::Null);
        if (200..300).contains(&status) && answer["ok"] != Value::Bool(false) {
            return Ok(answer.get("result").cloned().unwrap_or(Value::Null));
        }
        if already_done(method, &answer) {
            return Ok(Value::Null);
        }
        Err(error_for(method, status, &answer))
    }

    /// `getFile`, then the file URL. Refuses a file over [`DOWNLOAD_MAX`]
    /// before and after the download.
    pub(crate) async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>> {
        self.download_raw(file)
            .await
            .map_err(|e| redact(e, self.token))
    }

    async fn download_raw(&self, file: &IncomingFile) -> Result<Vec<u8>> {
        let too_big = |size: u64| {
            Error::Invalid(format!(
                "Telegram file is {size} bytes; bots may download at most {DOWNLOAD_MAX} \
                 (https://core.telegram.org/bots/api#getfile)"
            ))
        };
        if let Some(size) = file.size_bytes.filter(|s| *s > DOWNLOAD_MAX) {
            return Err(too_big(size));
        }
        let body = serde_json::json!({ "file_id": file.platform_file_id });
        let meta = self.call("getFile", &body).await?;
        if let Some(size) = meta["file_size"].as_u64().filter(|s| *s > DOWNLOAD_MAX) {
            return Err(too_big(size));
        }
        let path = meta["file_path"]
            .as_str()
            .ok_or_else(|| Error::Platform("Telegram getFile: no file_path".into()))?;
        let req = http::Request::get(format!("{API_BASE}/file/bot{}/{path}", self.token))
            .body(Vec::new())
            .map_err(|e| Error::Platform(format!("Telegram file download: {e}")))?;
        let resp = self.http.send(req).await?;
        if !resp.status().is_success() {
            return Err(Error::Platform(format!(
                "Telegram file download: {}",
                resp.status()
            )));
        }
        let bytes = resp.into_body();
        if bytes.len() as u64 > DOWNLOAD_MAX {
            return Err(too_big(bytes.len() as u64));
        }
        Ok(bytes)
    }
}

/// The `message_id` in a `send*` result, as text.
pub(crate) fn message_id(result: &Value) -> Option<String> {
    result["message_id"].as_i64().map(|id| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    const TOKEN: &str = "8000000001:AAFsealBayTestTokenNotReal0000000000";

    /// Fails like reqwest does: the error text quotes the request URL.
    struct Unreachable;

    #[async_trait]
    impl HttpClient for Unreachable {
        async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
            Err(Error::Platform(format!(
                "error sending request for url ({})",
                req.uri()
            )))
        }
    }

    fn tokenless(e: &Error) -> String {
        let text = e.to_string();
        assert!(!text.contains(TOKEN), "token leaked: {text}");
        assert!(!text.contains("AAFsealBay"), "token leaked: {text}");
        text
    }

    #[tokio::test]
    async fn port_errors_never_carry_the_token() {
        let api = Api {
            http: &Unreachable,
            token: TOKEN,
        };
        let e = api.call("sendMessage", &json!({})).await.unwrap_err();
        let text = tokenless(&e);
        assert!(text.contains("bot<bot-token>/sendMessage"), "{text}");
        let file = IncomingFile {
            platform_file_id: "f".into(),
            file_name: None,
            content_type: None,
            size_bytes: None,
        };
        tokenless(&api.download(&file).await.unwrap_err());
    }

    #[test]
    fn already_done_answers_are_success() {
        let same = json!({"ok": false, "error_code": 400, "description": "Bad Request: message is not modified: specified new message content and reply markup are exactly the same"});
        assert!(already_done("editMessageText", &same));
        let gone = json!({"ok": false, "error_code": 400, "description": "Bad Request: message to delete not found"});
        assert!(already_done("deleteMessage", &gone));
        assert!(!already_done("sendMessage", &gone));
    }

    #[test]
    fn errors_map_to_the_core() {
        let blocked = json!({"ok": false, "error_code": 403, "description": "Forbidden: bot was blocked by the user"});
        assert!(matches!(
            error_for("sendMessage", 403, &blocked),
            Error::Unreachable
        ));
        let flood = json!({"ok": false, "error_code": 429, "description": "Too Many Requests: retry after 7", "parameters": {"retry_after": 7}});
        let e = error_for("sendMessage", 429, &flood);
        assert!(matches!(
            e,
            Error::RateLimited {
                retry_after_secs: 7
            }
        ));
        assert_eq!(e.to_string(), "rate limited, retry after 7 s");
        let old = json!({"ok": false, "error_code": 400, "description": "Bad Request: message can't be deleted for everyone"});
        assert!(matches!(
            error_for("deleteMessage", 400, &old),
            Error::Unsupported(Capability::DeleteMessage)
        ));
        assert!(matches!(
            error_for("sendMessage", 400, &old),
            Error::Platform(_)
        ));
        let gone =
            json!({"ok": false, "error_code": 400, "description": "Bad Request: chat not found"});
        assert!(matches!(
            error_for("setMyCommands", 400, &gone),
            Error::Unreachable
        ));
        let other = json!({"ok": false, "error_code": 400, "description": "Bad Request: chat not found in the list"});
        assert!(matches!(
            error_for("setMyCommands", 400, &other),
            Error::Platform(_)
        ));
        assert!(matches!(
            error_for("sendMessage", 502, &Value::Null),
            Error::Platform(m) if m.contains("502")
        ));
    }
}
