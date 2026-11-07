//! Body limits of the back-office guard (#46): only the roster upload
//! route reads a large multipart body, a body over its limit answers 413
//! (or back to the upload form with "too large"), and Staff without the
//! Permission are refused before their body is read at all.

mod roster_support;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use harpseal_testing::{Browser, TestResponse};
use http::{Request, header};
use http_body::Frame;
use roster_support::*;

const BOUNDARY: &str = "----sealbay-boundary";

/// A multipart body with these `(name, value)` parts.
fn multipart_bytes(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in parts {
        let head = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"");
        body.extend_from_slice(format!("{head}\r\n\r\n").as_bytes());
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn post(b: &Browser<'_>, path: &str, body: Body, length: Option<usize>) -> TestResponse {
    let mut req = Request::post(path).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={BOUNDARY}"),
    );
    if let Some(n) = length {
        req = req.header(header::CONTENT_LENGTH, n);
    }
    b.send(req.body(body).unwrap()).await
}

/// A request body that records being polled and then fails.
struct Untouchable(Arc<AtomicBool>);

impl http_body::Body for Untouchable {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        self.0.store(true, Ordering::SeqCst);
        Poll::Ready(Some(Err(std::io::Error::other("the body was read"))))
    }
}

#[tokio::test]
async fn only_the_upload_route_reads_a_large_multipart_body() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let token = b.csrf().unwrap();
    // Another back-office form: 100 KiB is past the small generic limit.
    let padding = vec![b'x'; 100 * 1024];
    let bytes = multipart_bytes(&[("_csrf", token.as_bytes()), ("pad", &padding)]);
    let other = post(&b, "/admin/roster/none/cancel", Body::from(bytes), None).await;
    assert_eq!(other.status, 413, "{}", other.body);
    assert!(
        other.body.contains("ข้อมูลที่ส่งมีขนาดใหญ่เกินกว่าที่หน้านี้รับได้"),
        "{}",
        other.body
    );
    // A 200 KiB file is fine on the upload route.
    let big = csv(0..3000);
    assert!(big.len() > 200 * 1024);
    let r = upload(&b, "big.csv", &big, "upsert").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.roster.previewed");
}

#[tokio::test]
async fn an_upload_over_the_limit_goes_back_as_too_large() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let token = b.csrf().unwrap();
    let file = vec![b'x'; 5 * 1024 * 1024];
    let bytes = multipart_bytes(&[
        ("_csrf", token.as_bytes()),
        ("mode", b"full"),
        ("file", &file),
    ]);
    // Read until the limit, and refused by `Content-Length` before reading.
    for length in [None, Some(bytes.len())] {
        let body = Body::from(bytes.clone());
        let r = post(&b, "/admin/roster/upload", body, length).await;
        assert_eq!(r.status, 303, "{length:?}: {}", r.body);
        assert_eq!(r.header("location"), Some("/admin/roster"));
        assert_eq!(flash(&b), "err:kernel.roster.reason_too_large");
    }
    let page = b.get("/admin/roster").await;
    assert!(page.body.contains("ไฟล์ใหญ่เกิน 4 MB"), "{}", page.body);
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
    assert_eq!(h.files().keys(), Vec::<String>::new());
}

#[tokio::test]
async fn staff_without_the_permission_are_refused_before_the_body_is_read() {
    let (h, app) = boot().await;
    let viewer = signed_in(&h, &app, "Viewer").await;
    let polled = Arc::new(AtomicBool::new(false));
    let body = Body::new(Untouchable(polled.clone()));
    let r = post(&viewer, "/admin/roster/upload", body, Some(1024 * 1024)).await;
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(r.body.contains("kernel.roster.import"), "{}", r.body);
    assert!(!polled.load(Ordering::SeqCst), "the body was never read");
    // An Officer's body is read (and here fails), so the stream is real.
    let officer = signed_in(&h, &app, "Officer").await;
    let body = Body::new(Untouchable(polled.clone()));
    let r = post(&officer, "/admin/roster/upload", body, None).await;
    assert_eq!(r.status, 403);
    assert!(polled.load(Ordering::SeqCst));
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 0);
}
