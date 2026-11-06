//! A booted Deployment and its Router, driven in-process.

use std::sync::Arc;

use axum::body::Body;
use harpseal::{Deployment, Inbound, Rows, Statement};
use harpseal_channel_fake::{FakeChannel, SIGNATURE_HEADER};
use http::{HeaderMap, Request, StatusCode, header};
use tower::ServiceExt;

/// A Deployment with migrations applied, ready for requests.
pub struct TestApp {
    deployment: Arc<Deployment>,
    router: axum::Router,
    fake: FakeChannel,
}

/// A whole response: status, headers and the body as text.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TestResponse {
    /// HTTP status.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Body, decoded as UTF-8 (lossy).
    pub body: String,
}

impl TestResponse {
    /// A header value as text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

impl TestApp {
    pub(crate) fn new(d: Deployment, fake: FakeChannel) -> Self {
        let deployment = Arc::new(d);
        let router = deployment.clone().router();
        Self {
            deployment,
            router,
            fake,
        }
    }

    /// The Deployment behind the Router.
    pub fn deployment(&self) -> &Deployment {
        &self.deployment
    }

    /// The Router itself, for anything the helpers do not cover.
    pub fn router(&self) -> axum::Router {
        self.router.clone()
    }

    /// Sends one request through the Router.
    pub async fn request(&self, req: Request<Body>) -> TestResponse {
        let resp = self
            .router
            .clone()
            .oneshot(req)
            .await
            .unwrap_or_else(|e| match e {});
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body reads");
        TestResponse {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    /// `GET path`.
    pub async fn get(&self, path: &str) -> TestResponse {
        let req = Request::get(path)
            .body(Body::empty())
            .expect("valid request");
        self.request(req).await
    }

    /// `POST path` with an `application/x-www-form-urlencoded` body.
    pub async fn post_form(&self, path: &str, form: &str) -> TestResponse {
        let req = Request::post(path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form.to_string()))
            .expect("valid request");
        self.request(req).await
    }

    /// Posts one signed webhook call carrying `events` to `/webhook/fake`,
    /// as the platform would.
    pub async fn fake_webhook(&self, events: impl IntoIterator<Item = Inbound>) -> TestResponse {
        let events: Vec<Inbound> = events.into_iter().collect();
        let body = FakeChannel::encode(&events);
        let signature = self.fake.sign(&body);
        self.fake_webhook_raw(body, &signature).await
    }

    /// Posts `body` to `/webhook/fake` with `signature` as is (for bad
    /// signatures and unparseable bodies).
    pub async fn fake_webhook_raw(&self, body: Vec<u8>, signature: &str) -> TestResponse {
        let req = Request::post("/webhook/fake")
            .header(SIGNATURE_HEADER, signature)
            .body(Body::from(body))
            .expect("valid request");
        self.request(req).await
    }

    /// Runs a query on the Deployment's `Db` (read back what a later
    /// request would see).
    pub async fn query(&self, sql: &'static str) -> Rows {
        self.deployment
            .services()
            .db
            .query(Statement::new(sql))
            .await
            .expect("query runs")
    }

    /// One integer from a query (e.g. `SELECT count(*) ...`); 0 for none.
    pub async fn count(&self, sql: &'static str) -> i64 {
        let rows = self.query(sql).await;
        rows.rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }
}
