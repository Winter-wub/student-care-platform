//! A small browser over real HTTP: keeps cookies itself (so `Secure`
//! cookies work over `http://127.0.0.1`), scrapes the last `_csrf`, never
//! follows redirects (the walk checks each 303), and speaks the Fake
//! Channel's webhook and the example's `/dev` routes.

use std::collections::BTreeMap;
use std::sync::Mutex;

use harpseal::Inbound;
use harpseal_channel_fake::{FakeChannel, SIGNATURE_HEADER};
use serde_json::Value;

/// A step's error: what went wrong, for the table.
pub type R<T> = Result<T, String>;

/// One answer.
#[derive(Debug, Clone)]
pub struct Resp {
    /// HTTP status.
    pub status: u16,
    /// `Location`, on a redirect.
    pub location: Option<String>,
    /// The body as text.
    pub body: String,
}

impl Resp {
    /// Fails unless the status is `want`.
    pub fn expect(self, want: u16, what: &str) -> R<Self> {
        if self.status == want {
            return Ok(self);
        }
        let head: String = self.body.chars().take(200).collect();
        Err(format!(
            "{what}: status {} (want {want}): {head}",
            self.status
        ))
    }
    /// The `Location` of a 303.
    pub fn redirect(self, what: &str) -> R<String> {
        let r = self.expect(303, what)?;
        r.location
            .ok_or_else(|| format!("{what}: 303 without Location"))
    }
}

/// The server under test.
pub struct Client {
    http: reqwest::Client,
    base: String,
    fake: FakeChannel,
    cookies: Mutex<BTreeMap<String, String>>,
    csrf: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
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

/// The value of the first `name="_csrf"` input in `body`.
fn scrape_csrf(body: &str) -> Option<String> {
    let at = body.find("name=\"_csrf\"")?;
    let rest = &body[at..];
    let start = rest.find("value=\"")? + "value=\"".len();
    let end = rest[start..].find('"')?;
    Some(rest[start..start + end].to_string())
}

impl Client {
    /// A client for `base` (`http://127.0.0.1:<port>`) whose Fake Channel
    /// and `/dev` routes use `secret`.
    pub fn new(base: String, secret: &str) -> R<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let fake = FakeChannel::new(secret).map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            base,
            fake,
            cookies: Mutex::new(BTreeMap::new()),
            csrf: Mutex::new(None),
        })
    }

    /// The Fake Channel the server runs (same secret).
    pub fn fake(&self) -> &FakeChannel {
        &self.fake
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> R<Resp> {
        let jar = lock(&self.cookies)
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        let req = if jar.is_empty() {
            req
        } else {
            req.header("cookie", jar)
        };
        let resp = req.send().await.map_err(|e| e.without_url().to_string())?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        {
            let mut jar = lock(&self.cookies);
            for v in headers.get_all("set-cookie") {
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
        }
        if let Some(token) = scrape_csrf(&body) {
            *lock(&self.csrf) = Some(token);
        }
        let location = headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        Ok(Resp {
            status,
            location,
            body,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// `GET path`.
    pub async fn get(&self, path: &str) -> R<Resp> {
        self.send(self.http.get(self.url(path))).await
    }

    /// `POST path` with `form` as given.
    pub async fn post_raw(&self, path: &str, form: &str) -> R<Resp> {
        let req = self
            .http
            .post(self.url(path))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form.to_string());
        self.send(req).await
    }

    /// `POST path` with `form` and the last CSRF token seen.
    pub async fn post(&self, path: &str, form: &str) -> R<Resp> {
        let token = lock(&self.csrf).clone().unwrap_or_default();
        let sep = if form.is_empty() { "" } else { "&" };
        self.post_raw(path, &format!("{form}{sep}_csrf={}", urlencode(&token)))
            .await
    }

    /// `POST path` as `multipart/form-data`: the CSRF token, `fields`, then
    /// `file` (field `file`) named `name`.
    pub async fn post_file(
        &self,
        path: &str,
        fields: &[(&str, &str)],
        name: &str,
        file: &[u8],
    ) -> R<Resp> {
        let boundary = "----harpseal-e2e-boundary";
        let token = lock(&self.csrf).clone().unwrap_or_default();
        let mut body = Vec::new();
        let mut part = |head: String, value: &[u8]| {
            body.extend_from_slice(format!("--{boundary}\r\n{head}\r\n\r\n").as_bytes());
            body.extend_from_slice(value);
            body.extend_from_slice(b"\r\n");
        };
        let field = |n: &str| format!("Content-Disposition: form-data; name=\"{n}\"");
        part(field("_csrf"), token.as_bytes());
        for (k, v) in fields {
            part(field(k), v.as_bytes());
        }
        part(
            format!(
                "Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
                 Content-Type: application/octet-stream"
            ),
            file,
        );
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let req = self
            .http
            .post(self.url(path))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body);
        self.send(req).await
    }

    /// Delivers `events` to `/webhook/fake`, signed; fails unless 200.
    pub async fn webhook(&self, events: &[Inbound]) -> R<()> {
        let body = FakeChannel::encode(events);
        let req = self
            .http
            .post(self.url("/webhook/fake"))
            .header(SIGNATURE_HEADER, self.fake.sign(&body))
            .body(body);
        self.send(req).await?.expect(200, "webhook").map(|_| ())
    }

    async fn dev(&self, req: reqwest::RequestBuilder) -> R<Value> {
        let req = req.header("x-dev-secret", self.fake.secret());
        let r = self.send(req).await?.expect(200, "dev route")?;
        serde_json::from_str(&r.body).map_err(|e| format!("dev route JSON: {e}"))
    }

    /// `GET /dev/outbox?to=<external id>`: `{messages, menu}`.
    pub async fn outbox(&self, external_id: &str) -> R<Value> {
        let path = format!("/dev/outbox?to={}", urlencode(external_id));
        self.dev(self.http.get(self.url(&path))).await
    }

    /// `GET /dev/email?to=<address>`: the last email.
    pub async fn last_email(&self, to: &str) -> R<Value> {
        let path = format!("/dev/email?to={}", urlencode(to));
        self.dev(self.http.get(self.url(&path))).await
    }

    /// `POST /dev/tick`: runs the Jobs now; returns the TickReport JSON.
    pub async fn tick(&self) -> R<Value> {
        self.dev(self.http.post(self.url("/dev/tick"))).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_is_scraped_from_the_first_hidden_input() {
        let page = r#"<form><input type="hidden" name="_csrf" value="abc123"></form>"#;
        assert_eq!(scrape_csrf(page).as_deref(), Some("abc123"));
        assert_eq!(scrape_csrf("<p>no form</p>"), None);
        assert_eq!(urlencode("a b@ก"), "a%20b%40%E0%B8%81");
    }
}
