//! Does a request continue the Mini App launch its App Session came from
//! (#86)?
//!
//! The cookie alone does not say who holds the phone or the browser: on a
//! shared computer (Telegram Web on a lab PC, LIFF in an external browser)
//! the next person who opens the Mini App within the session's hour would
//! get the previous Student's pages. Only the platform's launch data
//! (LIFF ID token, Telegram `initData`) says who opened it, and only the
//! entry page's script can read it. So a request that does not come from a
//! page of this origin is a **new launch**: the chat app, LIFF's redirect,
//! web.telegram.org's iframe, a Card's deep link, a typed URL or a
//! bookmark. On a new launch [`super::pages::open`] drops the session (row
//! and cookie) before anything is read and answers the entry page, which
//! exchanges the current platform user's launch data for a new session.
//! A request from one of our own pages (a link, a form, HTMX, the
//! exchange's redirect) continues the launch and renders at once.
//!
//! Signals, in order:
//! - `Sec-Fetch-Site` (every current browser and webview): only
//!   `same-origin` continues; `cross-site`, `same-site` and `none` (the
//!   app opened the URL, or the user typed it) are new launches;
//! - without it (an older webview), the `Referer`: Mini App responses send
//!   `Referrer-Policy: same-origin`, so our own pages always name
//!   themselves and nobody else's do; it continues only when its host is
//!   the request's `Host`;
//! - neither: a new launch.
//!
//! The cost: one more round trip per launch (entry page, exchange, 303 to
//! the page: the same as a launch without a cookie), and on LINE one call
//! to LINE's verify endpoint per launch (counted by the exchange's limit);
//! navigating inside the Mini App costs nothing more. A reload counts as
//! a new launch when the browser sends `none` for it.
//!
//! This is not a defence against someone at the keyboard who wants the
//! previous Student's data on purpose: they hold the cookie and can send
//! any header. It stops the next honest user from being shown it.

use axum::http::{HeaderMap, header};

/// The request continues the current launch (see the module docs).
pub(crate) fn continues(headers: &HeaderMap) -> bool {
    match headers.get("sec-fetch-site") {
        Some(site) => site == "same-origin",
        None => referer_is_own(headers),
    }
}

/// The `Referer`'s host and port are the request's `Host`.
fn referer_is_own(headers: &HeaderMap) -> bool {
    let get = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(host), Some(referer)) = (get(header::HOST), get(header::REFERER)) else {
        return false;
    };
    let Some((scheme, rest)) = referer.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    matches!(scheme, "http" | "https") && !host.is_empty() && authority.eq_ignore_ascii_case(host)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::continues;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn only_a_same_origin_fetch_continues() {
        assert!(continues(&headers(&[("sec-fetch-site", "same-origin")])));
        for site in ["cross-site", "same-site", "none", ""] {
            assert!(!continues(&headers(&[("sec-fetch-site", site)])), "{site}");
        }
        // Fetch Metadata wins over the Referer.
        let h = headers(&[
            ("sec-fetch-site", "none"),
            ("host", "app.example"),
            ("referer", "https://app.example/app"),
        ]);
        assert!(!continues(&h));
        assert!(!continues(&HeaderMap::new()));
    }

    #[test]
    fn without_fetch_metadata_the_referer_must_be_this_host() {
        let own = |r: &str| continues(&headers(&[("host", "app.example:8443"), ("referer", r)]));
        assert!(own("https://app.example:8443/app/announcements"));
        assert!(own("http://APP.example:8443"));
        for other in [
            "https://app.example/app",
            "https://app.example:8443.evil.example/app",
            "https://web.telegram.org/",
            "https://liff.line.me/1234-abcd/app",
            "app.example:8443/app",
            "javascript://app.example:8443/",
            "",
        ] {
            assert!(!own(other), "{other}");
        }
        let no_host = headers(&[("referer", "https://app.example/app")]);
        assert!(!continues(&no_host));
    }
}
