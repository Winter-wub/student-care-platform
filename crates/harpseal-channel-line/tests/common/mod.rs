//! Shared by the LINE integration tests: a recording HttpClient that
//! answers like the Messaging API, signed webhook fixtures and PNG headers.

#![allow(dead_code)] // each test binary uses part of this

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harpseal::{HttpClient, Result, WebhookRequest};
use harpseal_testing::{RecordedRequest, RecordingHttp};
use serde_json::{Value, json};

pub const SECRET: &str = "seal-bay-line-channel-secret";
pub const TOKEN: &str = "seal-bay-access-token";
pub const USER: &str = "U5ea1ba7000000000000000000000c0de";

/// Records every request in a [`RecordingHttp`] (which also answers any
/// queued response) and replaces the answer of a 200 with what LINE would
/// send: ids for sent messages, a new `richMenuId`, the rich menus created
/// so far (less the deleted ones). Deleting or linking a rich menu that does
/// not exist answers 404, as LINE does. Queue a non-200 with
/// `http().respond_with` to fail a call.
#[derive(Clone, Default)]
pub struct LineApi {
    rec: RecordingHttp,
    state: Arc<Mutex<ApiState>>,
}

#[derive(Default)]
struct ApiState {
    next: u64,
    /// (richMenuId, created object), deleted ones included
    menus: Vec<(String, Value)>,
    deleted: Vec<String>,
    /// (userId, linked richMenuId)
    links: Vec<(String, String)>,
}

impl ApiState {
    fn live(&self, id: &str) -> bool {
        self.menus.iter().any(|(i, _)| i == id) && !self.deleted.iter().any(|d| d == id)
    }
}

impl LineApi {
    pub fn http(&self) -> &RecordingHttp {
        &self.rec
    }
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.rec.requests()
    }
    pub fn take(&self) -> Vec<RecordedRequest> {
        self.rec.take_requests()
    }
    /// The rich menu object created under `id`.
    pub fn created(&self, id: &str) -> Option<Value> {
        let s = self.state.lock().unwrap();
        s.menus
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, v)| v.clone())
    }
    /// Names of the rich menus that exist now, in creation order.
    pub fn live_names(&self) -> Vec<String> {
        let s = self.state.lock().unwrap();
        s.menus
            .iter()
            .filter(|(i, _)| s.live(i))
            .map(|(_, m)| m["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }
    /// The rich menu last linked to `user`.
    pub fn linked(&self, user: &str) -> Option<String> {
        let s = self.state.lock().unwrap();
        s.links
            .iter()
            .rev()
            .find(|(u, _)| u == user)
            .map(|(_, id)| id.clone())
    }
    /// Deletes rich menu `id` as someone in the LINE console would.
    pub fn delete_in_console(&self, id: &str) {
        self.state.lock().unwrap().deleted.push(id.into());
    }
    /// Pretends `obj` already exists on the account under `id`.
    pub fn preexisting(&self, id: &str, obj: Value) {
        self.state.lock().unwrap().menus.push((id.into(), obj));
    }
}

pub fn json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

#[async_trait]
impl HttpClient for LineApi {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        let uri = req.uri().to_string();
        let method = req.method().clone();
        let body = json(req.body());
        let resp = self.rec.send(req).await?;
        if resp.status() != 200 {
            return Ok(resp);
        }
        let mut s = self.state.lock().unwrap();
        let answer = if uri.ends_with("/v2/bot/message/reply") || uri.ends_with("/message/push") {
            let n = body["messages"].as_array().map_or(0, Vec::len);
            let ids: Vec<Value> = (0..n)
                .map(|_| {
                    s.next += 1;
                    json!({ "id": format!("5{:017}", s.next), "quoteToken": "q" })
                })
                .collect();
            json!({ "sentMessages": ids })
        } else if uri.ends_with("/v2/bot/richmenu") && method == http::Method::POST {
            s.next += 1;
            let id = format!("richmenu-{:032}", s.next);
            s.menus.push((id.clone(), body));
            json!({ "richMenuId": id })
        } else if uri.ends_with("/v2/bot/richmenu/list") {
            let list: Vec<Value> = s
                .menus
                .iter()
                .filter(|(id, _)| s.live(id))
                .map(|(id, m)| json!({ "richMenuId": id, "name": m["name"], "size": m["size"] }))
                .collect();
            json!({ "richmenus": list })
        } else if let Some(rest) = uri.strip_prefix("https://api.line.me/v2/bot/") {
            let parts: Vec<&str> = rest.split('/').collect();
            let target = match (method.as_str(), parts.as_slice()) {
                ("DELETE", ["richmenu", id]) => Some((None, id.to_string())),
                ("POST", ["user", user, "richmenu", id]) => {
                    Some((Some(user.to_string()), id.to_string()))
                }
                _ => None,
            };
            let Some((user, id)) = target else {
                return Ok(resp);
            };
            if !s.live(&id) {
                return Ok(http::Response::builder()
                    .status(404)
                    .body(br#"{"message":"Not found"}"#.to_vec())
                    .unwrap());
            }
            match user {
                Some(u) => s.links.push((u, id)),
                None => s.deleted.push(id),
            }
            return Ok(resp);
        } else {
            return Ok(resp);
        };
        Ok(http::Response::builder()
            .status(200)
            .body(answer.to_string().into_bytes())
            .unwrap())
    }
}

/// `body` signed with `secret` as LINE signs webhooks.
pub fn signed(body: &[u8], secret: &str) -> WebhookRequest {
    let mut headers = http::HeaderMap::new();
    let sig = harpseal_channel_line::sign(secret, body);
    headers.insert("x-line-signature", sig.parse().unwrap());
    WebhookRequest {
        headers,
        body: body.to_vec(),
    }
}

/// The raw recorded webhook body `tests/fixtures/webhook/<name>.json`.
pub fn fixture_body(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/webhook/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("fixture exists")
}

/// The fixture, signed with [`SECRET`].
pub fn fixture(name: &str) -> WebhookRequest {
    signed(&fixture_body(name), SECRET)
}

/// The first 24 bytes of a PNG of `w` x `h` (all the adapter reads).
pub fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 6, 0, 0, 0]);
    v
}
