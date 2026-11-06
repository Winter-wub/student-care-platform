//! A cookie-keeping client over a [`TestApp`], for back-office pages.

use std::collections::BTreeMap;
use std::sync::Mutex;

use axum::body::Body;
use harpseal_channel_fake::FakeChannel;
use http::{Request, header};

use crate::{TestApp, TestResponse};

/// One browser: keeps the cookies the Router sets (honouring `Max-Age=0`)
/// and the last CSRF token seen in a page, and sends both back.
///
/// ```no_run
/// # async fn demo(app: &harpseal_testing::TestApp) {
/// let b = app.browser();
/// b.login("admin@sealbay.example", "correct horse battery").await;
/// assert_eq!(b.get("/admin").await.status, 200);
/// b.post("/admin/logout", "").await; // adds `_csrf`
/// # }
/// ```
pub struct Browser<'a> {
    app: &'a TestApp,
    cookies: Mutex<BTreeMap<String, String>>,
    csrf: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl TestApp {
    /// A fresh browser with no cookies.
    pub fn browser(&self) -> Browser<'_> {
        Browser {
            app: self,
            cookies: Mutex::new(BTreeMap::new()),
            csrf: Mutex::new(None),
        }
    }
}

impl Browser<'_> {
    /// The value of cookie `name`, if the browser holds it.
    pub fn cookie(&self, name: &str) -> Option<String> {
        lock(&self.cookies).get(name).cloned()
    }

    /// Drops every cookie (a new browser session).
    pub fn clear_cookies(&self) {
        lock(&self.cookies).clear();
    }

    /// The last `_csrf` value seen in a page.
    pub fn csrf(&self) -> Option<String> {
        lock(&self.csrf).clone()
    }

    fn cookie_header(&self) -> String {
        let jar = lock(&self.cookies);
        let pairs: Vec<String> = jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
        pairs.join("; ")
    }

    /// Sends `req` with the cookies, then stores what comes back.
    pub async fn send(&self, mut req: Request<Body>) -> TestResponse {
        let cookies = self.cookie_header();
        if !cookies.is_empty() {
            let v = cookies.parse().expect("cookie header is ASCII");
            req.headers_mut().insert(header::COOKIE, v);
        }
        let resp = self.app.request(req).await;
        self.absorb(&resp);
        resp
    }

    fn absorb(&self, resp: &TestResponse) {
        let mut jar = lock(&self.cookies);
        for v in resp.headers.get_all(header::SET_COOKIE) {
            let Some((name, rest)) = v.to_str().ok().and_then(|v| v.split_once('=')) else {
                continue;
            };
            let value = rest.split(';').next().unwrap_or_default();
            if rest.contains("Max-Age=0") || value.is_empty() {
                jar.remove(name);
            } else {
                jar.insert(name.to_string(), value.to_string());
            }
        }
        if let Some(token) = scrape_csrf(&resp.body) {
            *lock(&self.csrf) = Some(token);
        }
    }

    /// `GET path`.
    pub async fn get(&self, path: &str) -> TestResponse {
        let req = Request::get(path)
            .body(Body::empty())
            .expect("valid request");
        self.send(req).await
    }

    /// `POST path` with `form` exactly as given (no CSRF token added).
    pub async fn post_raw(&self, path: &str, form: &str) -> TestResponse {
        let req = Request::post(path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form.to_string()))
            .expect("valid request");
        self.send(req).await
    }

    /// `POST path` with `form` plus the last CSRF token seen.
    pub async fn post(&self, path: &str, form: &str) -> TestResponse {
        let token = self.csrf().unwrap_or_default();
        let form = if form.is_empty() {
            format!("_csrf={token}")
        } else {
            format!("{form}&_csrf={token}")
        };
        self.post_raw(path, &form).await
    }

    /// Opens the Mini App as the entry page would once the Fake platform
    /// gave it a token: posts `fake`'s kind, its
    /// [`FakeChannel::mini_app_token`] for `external_id` and `next` to
    /// `/app/session`, and keeps the App Session cookie. Returns the POST's
    /// response (303 to `next` on success).
    pub async fn open_mini_app(
        &self,
        fake: &FakeChannel,
        external_id: &str,
        next: &str,
    ) -> TestResponse {
        let kind = harpseal::Channel::kind(fake);
        let token = fake.mini_app_token(external_id);
        self.open_mini_app_raw(kind.as_str(), &token, next).await
    }

    /// Posts `channel`, `token` and `next` to `/app/session` as given (real
    /// adapters' tokens, forged tokens).
    pub async fn open_mini_app_raw(&self, channel: &str, token: &str, next: &str) -> TestResponse {
        let form = format!(
            "channel={}&token={}&next={}",
            urlencode(channel),
            urlencode(token),
            urlencode(next)
        );
        self.post_raw("/app/session", &form).await
    }

    /// Opens the sign-in form and submits it; returns the POST's response
    /// (303 on success).
    pub async fn login(&self, email: &str, password: &str) -> TestResponse {
        self.get("/admin/login").await;
        let form = format!(
            "email={}&password={}",
            urlencode(email),
            urlencode(password)
        );
        self.post("/admin/login", &form).await
    }
}

/// The value of the first `name="_csrf"` input in `body`.
fn scrape_csrf(body: &str) -> Option<String> {
    let at = body.find("name=\"_csrf\"")?;
    let rest = &body[at..];
    let start = rest.find("value=\"")? + "value=\"".len();
    let end = rest[start..].find('"')?;
    Some(rest[start..start + end].to_string())
}

/// Percent-encodes everything but ASCII letters and digits.
pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
