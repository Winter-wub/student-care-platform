//! [`WorkerHttp`]: the `HttpClient` port over `fetch`.
//!
//! Errors never carry the request URL: a JS error can quote it ("Invalid
//! URL: …", "Fetch API cannot load …"), and adapters put secrets in URLs
//! (Telegram's `…/bot<token>/…`). Every error is scrubbed with
//! [`scrub_url`] before it becomes an [`Error`], as the native runtime
//! strips reqwest errors with `without_url`; a header that cannot be set
//! is named, its value (maybe a credential) never quoted.

use async_trait::async_trait;
use harpseal::{Error, Result};
use worker::js_sys::Uint8Array;
use worker::send::SendFuture;
use worker::{Fetch, Headers, Method, Request, RequestInit};

use crate::platform;

/// Outgoing HTTP through `fetch`. Each call is one subrequest (Workers
/// Paid allows 1000 per request, Free 50).
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerHttp;

/// What replaces a URL (or a part of one) in an error message.
const REDACTED: &str = "[url]";

/// `msg` without `url`, its path and query, anything that looks like a
/// URL (`scheme://…` up to whitespace or a quote/bracket) or a Telegram
/// bot token (`bot<digits>:<secret>`), so a token cannot leak through an
/// error into logs or Security Events.
pub(crate) fn scrub_url(msg: &str, uri: &http::Uri) -> String {
    let mut out = msg.to_string();
    let whole = uri.to_string();
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("");
    for part in [whole.as_str(), path] {
        if part.len() > 1 {
            out = out.replace(part, REDACTED);
        }
    }
    scrub_bot_tokens(&scrub_schemes(&out))
}

/// Replaces every Telegram bot token (`bot<digits>:<secret>`) with
/// [`REDACTED`], wherever it stands (a JS error may quote a bare path).
fn scrub_bot_tokens(msg: &str) -> String {
    let secret = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(at) = rest.find("bot") {
        let tail = &rest[at + 3..];
        let digits = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let after = &tail[digits..];
        if digits > 0 && after.starts_with(':') {
            let len = after[1..]
                .find(|c: char| !secret(c))
                .unwrap_or(after.len() - 1);
            if len > 0 {
                out.push_str(&rest[..at]);
                out.push_str(REDACTED);
                rest = &after[1 + len..];
                continue;
            }
        }
        out.push_str(&rest[..at + 3]);
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Replaces every `scheme://rest` run with [`REDACTED`].
fn scrub_schemes(msg: &str) -> String {
    let stop = |c: char| c.is_whitespace() || "\"'`<>()[]{}".contains(c);
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(at) = rest.find("://") {
        // The scheme: the ASCII letters (and + . -) right before `://`.
        let head = &rest[..at];
        let start = head
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || "+.-".contains(*c))
            .last()
            .map_or(at, |(i, _)| i);
        out.push_str(&rest[..start]);
        out.push_str(REDACTED);
        let tail = &rest[at + 3..];
        let end = tail.find(stop).unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[async_trait]
impl harpseal::HttpClient for WorkerHttp {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        SendFuture::new(async move {
            let (parts, body) = req.into_parts();
            let uri = parts.uri.clone();
            let transport = |e: worker::Error| Error::Platform(scrub_url(&e.to_string(), &uri));
            let headers = Headers::new();
            for (k, v) in &parts.headers {
                // The value may be a credential (`Authorization`): name
                // the header, never quote it.
                let bad = || Error::Platform(format!("header {k} could not be set"));
                let v = v.to_str().map_err(|_| bad())?;
                headers.append(k.as_str(), v).map_err(|_| bad())?;
            }
            let mut init = RequestInit::new();
            init.with_method(Method::from(parts.method.to_string()))
                .with_headers(headers);
            if !body.is_empty() {
                init.with_body(Some(Uint8Array::from(body.as_slice()).into()));
            }
            let req = Request::new_with_init(&uri.to_string(), &init).map_err(transport)?;
            let mut resp = Fetch::Request(req).send().await.map_err(transport)?;
            let mut out = http::Response::builder().status(resp.status_code());
            for (k, v) in resp.headers().entries() {
                out = out.header(k, v);
            }
            let bytes = resp.bytes().await.map_err(transport)?;
            out.body(bytes).map_err(platform)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "123456:AAH-secret_token";

    fn uri() -> http::Uri {
        format!("https://api.telegram.org/bot{TOKEN}/sendMessage?x=1")
            .parse()
            .unwrap()
    }

    #[test]
    fn a_quoted_url_loses_its_token() {
        let u = uri();
        for msg in [
            format!("TypeError: Invalid URL: {u}"),
            format!("Fetch API cannot load: \"{u}\"."),
            format!("Invalid URL string: /bot{TOKEN}/sendMessage?x=1"),
            // Another rendering of the same URL (no query, http).
            format!("network error (http://api.telegram.org/bot{TOKEN}/x)"),
        ] {
            let out = scrub_url(&msg, &u);
            assert!(!out.contains(TOKEN), "{out}");
            assert!(!out.contains("bot123456"), "{out}");
            assert!(out.contains(REDACTED), "{out}");
        }
        assert_eq!(
            scrub_url(&format!("TypeError: Invalid URL: {u}"), &u),
            "TypeError: Invalid URL: [url]"
        );
    }

    #[test]
    fn a_fabricated_js_error_with_a_bot_token_keeps_neither_url_nor_token() {
        // Another request's URL than the one sent, and a bare token: only
        // the patterns can catch them.
        let js = "TypeError: Failed to parse URL from https://evil.invalid/x?t=1 \
                  (bot987654:ZZZ-fake_Token9 in /bot987654:ZZZ-fake_Token9/getMe)";
        let out = scrub_url(js, &uri());
        assert!(!out.contains("ZZZ-fake_Token9"), "{out}");
        assert!(!out.contains("https://"), "{out}");
        assert!(!out.contains("evil.invalid"), "{out}");
        assert_eq!(
            scrub_bot_tokens("robot:1 bot12: bot:x"),
            "robot:1 bot12: bot:x"
        );
    }

    #[test]
    fn a_message_without_a_url_is_kept() {
        let msg = "TypeError: Network connection lost.";
        assert_eq!(scrub_url(msg, &uri()), msg);
        assert_eq!(scrub_schemes("a://b c"), "[url] c");
        assert_eq!(scrub_schemes("ends with ://"), "ends with [url]");
    }
}
