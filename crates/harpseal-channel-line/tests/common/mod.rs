//! Shared by the LINE integration tests: a recording HttpClient that
//! answers like the Messaging API, signed webhook fixtures and PNG headers.

#![allow(dead_code)] // each test binary uses part of this

pub mod relink;

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
    /// Phases the progress endpoint answers before `succeeded`.
    phases: std::collections::VecDeque<String>,
    /// (request id, [(from, to)]) of each batch control request.
    batches: Vec<(String, Vec<(String, String)>)>,
    /// User ids a bulk link refuses with a 400 naming their position.
    refused: Vec<String>,
    /// Bulk links answer 400 with this message (naming no user); per-user
    /// links too when the flag is set.
    refuse_links: Option<(String, bool)>,
    /// Rich menus that have their image; linking any other answers 400.
    images: Vec<String>,
}

impl ApiState {
    fn live(&self, id: &str) -> bool {
        self.menus.iter().any(|(i, _)| i == id) && !self.deleted.iter().any(|d| d == id)
    }
    fn imaged(&self, id: &str) -> bool {
        self.images.iter().any(|i| i == id)
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
    /// A bulk link with `user` in it answers 400 naming its position, as
    /// LINE does for an invalid user id.
    pub fn refuse_user(&self, user: &str) {
        self.state.lock().unwrap().refused.push(user.into());
    }
    /// Every bulk link answers 400 with `message` (naming no user), and
    /// every per-user link too when `per_user`; `None` stops it.
    pub fn refuse_links(&self, refusal: Option<(&str, bool)>) {
        self.state.lock().unwrap().refuse_links = refusal.map(|(m, p)| (m.to_string(), p));
    }
    /// The progress endpoint answers `phase` once before `succeeded`.
    pub fn queue_phase(&self, phase: &str) {
        self.state.lock().unwrap().phases.push_back(phase.into());
    }
    /// The batch control requests: (request id, [(from, to)]).
    pub fn batches(&self) -> Vec<(String, Vec<(String, String)>)> {
        self.state.lock().unwrap().batches.clone()
    }
    /// The id of the live rich menu named `name`.
    pub fn id_of(&self, name: &str) -> Option<String> {
        let s = self.state.lock().unwrap();
        s.menus
            .iter()
            .find(|(i, m)| s.live(i) && m["name"] == name)
            .map(|(i, _)| i.clone())
    }
    /// Links `user` to rich menu `id` as an earlier `set_menu` did.
    pub fn link_user(&self, user: &str, id: &str) {
        self.state
            .lock()
            .unwrap()
            .links
            .push((user.into(), id.into()));
    }
    /// Deletes rich menu `id` as someone in the LINE console would.
    pub fn delete_in_console(&self, id: &str) {
        self.state.lock().unwrap().deleted.push(id.into());
    }
    /// Pretends `obj` already exists on the account under `id`, with its image.
    pub fn preexisting(&self, id: &str, obj: Value) {
        let mut s = self.state.lock().unwrap();
        s.menus.push((id.into(), obj));
        s.images.push(id.into());
    }
    /// [`LineApi::preexisting`], but created without an image (an upload
    /// that never happened): linking it answers 400, downloading its image
    /// 404, until the image is uploaded.
    pub fn preexisting_imageless(&self, id: &str, obj: Value) {
        self.state.lock().unwrap().menus.push((id.into(), obj));
    }
    /// Uploads an image to rich menu `id` as another isolate would.
    pub fn give_image(&self, id: &str) {
        self.state.lock().unwrap().images.push(id.into());
    }
    /// Whether rich menu `id` has an image.
    pub fn has_image(&self, id: &str) -> bool {
        self.state.lock().unwrap().imaged(id)
    }
}

/// LINE's 400 for linking a rich menu that has no image (the message the
/// docs show for the same condition when setting the default rich menu).
fn no_image() -> http::Response<Vec<u8>> {
    let body =
        r#"{"message":"must upload richmenu image before applying it to user","details":[]}"#;
    http::Response::builder()
        .status(400)
        .body(body.as_bytes().to_vec())
        .unwrap()
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
        let accepted = |headers: &[(&str, String)]| {
            let mut b = http::Response::builder().status(202);
            for (k, v) in headers {
                b = b.header(*k, v);
            }
            Ok(b.body(b"{}".to_vec()).unwrap())
        };
        let refusal = |msg: &str| {
            let body = serde_json::json!({ "message": msg }).to_string();
            Ok(http::Response::builder()
                .status(400)
                .body(body.into_bytes())
                .unwrap())
        };
        if let Some((msg, per_user)) = s.refuse_links.clone() {
            let per_user_link = method == http::Method::POST
                && uri.starts_with("https://api.line.me/v2/bot/user/")
                && uri.contains("/richmenu/");
            if uri.ends_with("/v2/bot/richmenu/bulk/link") || (per_user && per_user_link) {
                return refusal(&msg);
            }
        }
        if uri.ends_with("/v2/bot/richmenu/bulk/link") {
            let id = body["richMenuId"].as_str().unwrap_or_default().to_string();
            if !s.live(&id) {
                return Ok(http::Response::builder()
                    .status(404)
                    .body(br#"{"message":"richmenu not found","details":[]}"#.to_vec())
                    .unwrap());
            }
            if !s.imaged(&id) {
                return Ok(no_image());
            }
            let users = body["userIds"].as_array().cloned().unwrap_or_default();
            if let Some(n) = users
                .iter()
                .position(|u| s.refused.iter().any(|r| u == r.as_str()))
            {
                let msg = format!(
                    r#"{{"message":"The property, 'userIds[{n}]', in the request body is invalid (line: -, column: -)"}}"#
                );
                return Ok(http::Response::builder()
                    .status(400)
                    .body(msg.into_bytes())
                    .unwrap());
            }
            for u in body["userIds"].as_array().into_iter().flatten() {
                let u = u.as_str().unwrap_or_default().to_string();
                s.links.push((u, id.clone()));
            }
            return accepted(&[]);
        }
        if uri.ends_with("/v2/bot/richmenu/batch") {
            s.next += 1;
            let request = format!("req-{}", s.next);
            let mut ops = Vec::new();
            for op in body["operations"].as_array().into_iter().flatten() {
                let from = op["from"].as_str().unwrap_or_default().to_string();
                let to = op["to"].as_str().unwrap_or_default().to_string();
                let users: Vec<String> = s
                    .links
                    .iter()
                    .map(|(u, _)| u.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .filter(|u| {
                        s.links.iter().rev().find(|(x, _)| x == u).map(|(_, i)| i) == Some(&from)
                    })
                    .collect();
                for u in users {
                    s.links.push((u, to.clone()));
                }
                ops.push((from, to));
            }
            s.batches.push((request.clone(), ops));
            return accepted(&[("x-line-request-id", request)]);
        }
        if uri.contains("/v2/bot/richmenu/progress/batch") {
            let phase = s.phases.pop_front().unwrap_or_else(|| "succeeded".into());
            let answer = json!({ "phase": phase, "acceptedTime": "2026-11-07T03:00:00.000Z" });
            return Ok(http::Response::builder()
                .status(200)
                .body(answer.to_string().into_bytes())
                .unwrap());
        }
        if let Some(id) = uri
            .strip_prefix("https://api-data.line.me/v2/bot/richmenu/")
            .and_then(|r| r.strip_suffix("/content"))
        {
            let id = id.to_string();
            let found = |status: u16, body: Vec<u8>| {
                Ok(http::Response::builder().status(status).body(body).unwrap())
            };
            if !s.live(&id) {
                return found(404, br#"{"message":"Not found"}"#.to_vec());
            }
            if method == http::Method::POST {
                // LINE never replaces an image.
                if s.imaged(&id) {
                    let m = r#"{"message":"An image has already been uploaded to the richmenu"}"#;
                    return found(400, m.as_bytes().to_vec());
                }
                s.images.push(id);
                return found(200, b"{}".to_vec());
            }
            return match s.imaged(&id) {
                true => found(200, png(2500, 843)),
                false => found(404, br#"{"message":"Not found"}"#.to_vec()),
            };
        }
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
            if user.is_some() && !s.imaged(&id) {
                return Ok(no_image());
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
