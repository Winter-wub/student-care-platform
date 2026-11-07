//! Fake Services for tests: every one is cheap to clone and clones share
//! state, so the test keeps a handle after the Deployment takes one.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use harpseal::{
    Clock, Email, EmailSender, Error, FileStore, HttpClient, Random, Result, Timestamp,
};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A Clock that only moves when the test moves it.
#[derive(Debug, Clone)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    /// Starts at `ms` (milliseconds UTC).
    pub fn new(ms: i64) -> Self {
        Self(Arc::new(AtomicI64::new(ms)))
    }
    /// Now, in milliseconds.
    pub fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    /// Jumps to `ms`.
    pub fn set_ms(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
    /// Moves forward by `ms`.
    pub fn advance_ms(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
    /// Moves forward by whole minutes (OTP expiry, locks).
    pub fn advance_minutes(&self, minutes: i64) {
        self.advance_ms(minutes * 60_000);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.now_ms())
    }
}

/// Deterministic "random" bytes (SplitMix64 from a seed): the same test
/// gets the same ids and tokens on every run. Never use outside tests.
#[derive(Debug, Clone)]
pub struct SeqRandom(Arc<Mutex<u64>>);

impl SeqRandom {
    /// A stream starting at `seed`.
    pub fn new(seed: u64) -> Self {
        Self(Arc::new(Mutex::new(seed)))
    }
}

impl Random for SeqRandom {
    fn fill(&self, buf: &mut [u8]) -> Result<()> {
        let mut state = lock(&self.0);
        for chunk in buf.chunks_mut(8) {
            *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = *state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

/// An EmailSender that keeps every email instead of sending it. Addresses
/// marked with [`CapturingEmail::fail_for`] are refused (not kept).
#[derive(Debug, Clone, Default)]
pub struct CapturingEmail {
    sent: Arc<Mutex<Vec<Email>>>,
    failing: Arc<Mutex<Vec<String>>>,
}

impl CapturingEmail {
    /// Every email sent so far, in order.
    pub fn sent(&self) -> Vec<Email> {
        lock(&self.sent).clone()
    }
    /// The most recent email, if any.
    pub fn last(&self) -> Option<Email> {
        lock(&self.sent).last().cloned()
    }
    /// Emails sent to `to`.
    pub fn to(&self, to: &str) -> Vec<Email> {
        lock(&self.sent)
            .iter()
            .filter(|e| e.to == to)
            .cloned()
            .collect()
    }
    /// Makes every later send to `to` fail with `Error::Platform`
    /// (`false` undoes it).
    pub fn fail_for(&self, to: &str, failing: bool) {
        let mut f = lock(&self.failing);
        f.retain(|a| a != to);
        if failing {
            f.push(to.to_string());
        }
    }
}

#[async_trait]
impl EmailSender for CapturingEmail {
    async fn send(&self, email: Email) -> Result<()> {
        if lock(&self.failing).contains(&email.to) {
            return Err(Error::Platform(format!(
                "refused by the test: {}",
                email.to
            )));
        }
        lock(&self.sent).push(email);
        Ok(())
    }
}

/// One request the RecordingHttp saw.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecordedRequest {
    /// Method.
    pub method: http::Method,
    /// Full URI.
    pub uri: String,
    /// Headers.
    pub headers: http::HeaderMap,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl RecordedRequest {
    /// The body as UTF-8 (lossy).
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Debug, Default)]
struct HttpState {
    requests: Vec<RecordedRequest>,
    responses: VecDeque<(u16, Vec<u8>)>,
}

/// An HttpClient that records requests and answers from a queue of canned
/// responses, `200 {}` when the queue is empty. Nothing leaves the process.
#[derive(Debug, Clone, Default)]
pub struct RecordingHttp(Arc<Mutex<HttpState>>);

impl RecordingHttp {
    /// Queues the answer to the next unanswered request.
    pub fn respond_with(&self, status: u16, body: impl Into<Vec<u8>>) {
        lock(&self.0).responses.push_back((status, body.into()));
    }
    /// Every request so far, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.0).requests.clone()
    }
    /// Every request so far, and forgets them.
    pub fn take_requests(&self) -> Vec<RecordedRequest> {
        std::mem::take(&mut lock(&self.0).requests)
    }
}

#[async_trait]
impl HttpClient for RecordingHttp {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        let (parts, body) = req.into_parts();
        let mut s = lock(&self.0);
        s.requests.push(RecordedRequest {
            method: parts.method,
            uri: parts.uri.to_string(),
            headers: parts.headers,
            body,
        });
        let (status, body) = s.responses.pop_front().unwrap_or((200, b"{}".to_vec()));
        http::Response::builder()
            .status(status)
            .body(body)
            .map_err(|e| Error::Platform(e.to_string()))
    }
}

/// Stored bytes and content type per key.
type Objects = HashMap<String, (Vec<u8>, String)>;

/// A FileStore in memory. Like the real stores, it refuses a key that
/// fails [`harpseal::check_file_key`] (the empty key included).
#[derive(Debug, Clone, Default)]
pub struct MemoryFiles(Arc<Mutex<Objects>>, Arc<Mutex<Vec<String>>>);

impl MemoryFiles {
    /// Stored keys, sorted.
    pub fn keys(&self) -> Vec<String> {
        let mut k: Vec<String> = lock(&self.0).keys().cloned().collect();
        k.sort();
        k
    }

    /// Makes every later call on `key` fail (a platform error), as an
    /// object store that is down for one object.
    pub fn fail_on(&self, key: &str) {
        lock(&self.1).push(key.to_string());
    }

    fn check(&self, key: &str) -> Result<()> {
        harpseal::check_file_key(key)?;
        if lock(&self.1).iter().any(|k| k == key) {
            return Err(harpseal::Error::Platform(format!(
                "injected failure for {key}"
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl FileStore for MemoryFiles {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        self.check(key)?;
        lock(&self.0).insert(key.into(), (bytes, content_type.into()));
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.check(key)?;
        Ok(lock(&self.0).get(key).map(|(b, _)| b.clone()))
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.check(key)?;
        lock(&self.0).remove(key);
        Ok(())
    }
}
