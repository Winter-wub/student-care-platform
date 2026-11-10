//! Recorded-HTTP walk on LINE: a Student follows the bot and the locked
//! rich menu is created and linked, then links their account and the
//! unlocked one is, each on the configured image. Nothing leaves the
//! process: the Deployment runs on the harness's recording HttpClient, and
//! LINE's webhooks are signed and posted to its Router.

use harpseal::web::axum;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use example_deployment::menu_images::{LOCKED_PNG, LOCKED_VAR, UNLOCKED_PNG, UNLOCKED_VAR};
use example_deployment::{Secrets, builder};
use harpseal::{HttpClient, Locale, NewStaff, Statement};
use harpseal_channel_line::{SIGNATURE_HEADER, sign};
use harpseal_testing::{Harness, RecordedRequest, RecordingHttp, TestApp};
use serde_json::{Value, json};

const SECRET: &str = "seal-bay-line-channel-secret";
const USER: &str = "U5ea1ba7000000000000000000000c0de";

/// Records every call and answers the two LINE would answer with data:
/// the id of a created rich menu and the list of those created.
#[derive(Clone, Default)]
struct LineApi {
    rec: RecordingHttp,
    menus: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl HttpClient for LineApi {
    async fn send(&self, req: http::Request<Vec<u8>>) -> harpseal::Result<http::Response<Vec<u8>>> {
        let (method, uri) = (req.method().clone(), req.uri().to_string());
        let body: Value = serde_json::from_slice(req.body()).unwrap_or(Value::Null);
        let resp = self.rec.send(req).await?;
        let mut menus = self.menus.lock().unwrap();
        let answer = if method == http::Method::POST && uri.ends_with("/v2/bot/richmenu") {
            let id = format!("richmenu-{:032x}", menus.len() + 1);
            menus.push((id.clone(), body["name"].as_str().unwrap_or("").to_string()));
            json!({ "richMenuId": id })
        } else if method == http::Method::GET && uri.ends_with("/v2/bot/richmenu/list") {
            let list = menus
                .iter()
                .map(|(id, name)| json!({ "richMenuId": id, "name": name }));
            json!({ "richmenus": list.collect::<Vec<_>>() })
        } else {
            return Ok(resp);
        };
        Ok(http::Response::builder()
            .status(200)
            .body(answer.to_string().into_bytes())
            .unwrap())
    }
}

/// A Deployment with the LINE Channel on `secrets` (real credentials) and
/// one active Student in the roster.
async fn boot(h: &Harness, api: &LineApi, secrets: Secrets) -> TestApp {
    let mut services = h.services();
    services.http = Arc::new(api.clone());
    let d = builder(services, secrets)
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let student = "INSERT INTO student (id, student_code, display_name, email, locale, status, \
                   created_at, updated_at) VALUES ('s1', '68010001', 'Nok Thale', \
                   's68010001@student.sealbay.example', 'th', 'active', 1, 1)";
    h.db().execute(Statement::new(student)).await.unwrap();
    app
}

fn secrets(files: &[(&str, &str)]) -> Secrets {
    let env = |k: &str| {
        Some(match k {
            "LINE_CHANNEL_SECRET" => SECRET.to_string(),
            "LINE_ACCESS_TOKEN" => "seal-bay-access-token".to_string(),
            "LINE_LIFF_ID" => "1234567890-AbCdEfGh".to_string(),
            "TELEGRAM_WEB_APP_URL" => "https://app.seal-bay.invalid/tg".to_string(),
            other => files.iter().find(|(var, _)| *var == other)?.1.to_string(),
        })
    };
    Secrets::from_lookup(env)
}

/// One signed webhook event from `USER`.
async fn event(h: &Harness, app: &TestApp, n: u32, kind: Value) {
    let mut ev = json!({
        "webhookEventId": format!("01JSEALBAY00000000000000{n:02}"),
        "timestamp": h.clock().now_ms(),
        "mode": "active",
        "source": { "type": "user", "userId": USER },
        "replyToken": format!("reply-token-{n}"),
        "deliveryContext": { "isRedelivery": false },
    });
    ev.as_object_mut()
        .unwrap()
        .extend(kind.as_object().unwrap().clone());
    let body = json!({ "destination": "Ubot", "events": [ev] })
        .to_string()
        .into_bytes();
    let req = http::Request::post("/webhook/line")
        .header(SIGNATURE_HEADER, sign(SECRET, &body))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let r = app.request(req).await;
    assert_eq!(r.status, 200, "{}", r.body);
}

/// The rich menu calls among `reqs`, in order, as `(method, path)`.
fn rich_menu_calls(reqs: &[RecordedRequest]) -> Vec<(String, String)> {
    reqs.iter()
        .filter(|r| r.uri.contains("/richmenu"))
        .map(|r| {
            (
                r.method.to_string(),
                r.uri.split("line.me").nth(1).unwrap().to_string(),
            )
        })
        .collect()
}

/// Follow, then link through the consent card, student id and email code.
/// Returns the calls made by the follow and by the link.
async fn walk(
    h: &Harness,
    app: &TestApp,
    api: &LineApi,
) -> (Vec<RecordedRequest>, Vec<RecordedRequest>) {
    event(h, app, 1, json!({ "type": "follow" })).await;
    let followed = api.rec.take_requests();
    let say = |n: u32, text: &str| {
        (
            n,
            json!({ "type": "message", "message": { "type": "text", "id": n.to_string(), "text": text } }),
        )
    };
    let accept = json!({ "type": "postback", "postback": { "data": "kernel.consent.accept" } });
    event(h, app, 2, accept).await;
    let (n, ask) = say(3, "68010001");
    event(h, app, n, ask).await;
    let email = h.email().last().expect("an OTP email was sent");
    let code = email.subject.rsplit(' ').next().unwrap().to_string();
    let (n, code) = say(4, &code);
    event(h, app, n, code).await;
    (followed, api.rec.take_requests())
}

/// The body of the upload to `richmenu/{id}/content` among `reqs`.
fn uploaded(reqs: &[RecordedRequest]) -> Vec<u8> {
    let r = reqs
        .iter()
        .find(|r| {
            r.uri
                .starts_with("https://api-data.line.me/v2/bot/richmenu/")
                && r.uri.ends_with("/content")
        })
        .expect("an image was uploaded");
    assert_eq!(r.headers["content-type"], "image/png");
    r.body.clone()
}

/// The rich menu object sent to `POST /v2/bot/richmenu` among `reqs`.
fn created(reqs: &[RecordedRequest]) -> Value {
    let r = reqs
        .iter()
        .find(|r| r.uri == "https://api.line.me/v2/bot/richmenu")
        .expect("a rich menu was created");
    serde_json::from_slice(&r.body).unwrap()
}

#[tokio::test]
async fn following_sets_the_locked_rich_menu_and_linking_the_unlocked_one() {
    let h = Harness::new();
    let api = LineApi::default();
    let app = boot(&h, &api, secrets(&[])).await;
    let (followed, linked) = walk(&h, &app, &api).await;

    // Follow: the locked Menu (one "เริ่มใช้งาน" cell) on the locked image.
    let calls = rich_menu_calls(&followed);
    let verbs: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(&verbs[..4], ["GET", "POST", "POST", "POST"], "{calls:?}");
    let locked = created(&followed);
    assert!(
        locked["name"]
            .as_str()
            .unwrap()
            .starts_with("harpseal-locked-1-"),
        "{locked}"
    );
    assert_eq!(locked["size"], json!({ "width": 2500, "height": 843 }));
    let areas = locked["areas"].as_array().unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(areas[0]["action"]["data"], "kernel.link.start");
    assert_eq!(areas[0]["action"]["label"], "เริ่มใช้งาน");
    assert_eq!(
        areas[0]["bounds"],
        json!({ "x": 0, "y": 0, "width": 2500, "height": 843 })
    );
    assert_eq!(uploaded(&followed), LOCKED_PNG);
    let link = followed
        .iter()
        .find(|r| r.uri.contains(&format!("/user/{USER}/richmenu/")))
        .unwrap();
    assert!(
        link.uri
            .ends_with("richmenu-00000000000000000000000000000001"),
        "{}",
        link.uri
    );

    // Link: the default Locale's unlocked Menu (3 cells) on the unlocked image.
    let unlocked = created(&linked);
    assert!(
        unlocked["name"]
            .as_str()
            .unwrap()
            .starts_with("harpseal-th-1-"),
        "{unlocked}"
    );
    assert_eq!(unlocked["size"], json!({ "width": 2500, "height": 843 }));
    let data: Vec<&str> = unlocked["areas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["action"]["data"].as_str().unwrap())
        .collect();
    assert_eq!(
        data,
        ["announcements.latest", "contact.show", "kernel.link.unlink"]
    );
    assert_eq!(unlocked["areas"][1]["bounds"]["x"], 833);
    assert_eq!(uploaded(&linked), UNLOCKED_PNG);
    assert!(linked.iter().any(|r| r.uri.contains(&format!(
        "/user/{USER}/richmenu/richmenu-0000000000000000000000000000000"
    ))));

    // Nothing failed: the Inbounds were answered, and the Student is linked.
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1
    );
}

#[tokio::test]
async fn configured_files_are_what_gets_uploaded() {
    let h = Harness::new();
    let api = LineApi::default();
    let mine = |marker: &[u8], height: u32| {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        v.extend(2500u32.to_be_bytes());
        v.extend(height.to_be_bytes());
        v.extend([8, 6, 0, 0, 0]);
        v.extend(marker);
        v
    };
    let (locked_png, unlocked_png) = (mine(b"my locked", 843), mine(b"my unlocked", 1686));
    let read = |p: &str| match p {
        "/srv/locked.png" => Ok(locked_png.clone()),
        "/srv/unlocked.png" => Ok(unlocked_png.clone()),
        _ => Err(std::io::Error::other("unexpected path")),
    };
    let files = [
        (LOCKED_VAR, "/srv/locked.png"),
        (UNLOCKED_VAR, "/srv/unlocked.png"),
    ];
    let get = |k: &str| {
        files
            .iter()
            .find(|(v, _)| *v == k)
            .map(|(_, p)| p.to_string())
    };
    let configured = secrets(&[]).with_menu_image_files(get, read).unwrap();
    let app = boot(&h, &api, configured).await;
    let (followed, linked) = walk(&h, &app, &api).await;

    assert_eq!(uploaded(&followed), locked_png);
    assert_eq!(uploaded(&linked), unlocked_png);
    // The rich menu object follows the image it is drawn on: 1686 px high
    // (this image) with the same three cells.
    assert_eq!(created(&linked)["size"]["height"], 1686);
    assert_eq!(created(&linked)["areas"].as_array().unwrap().len(), 3);
}

/// `/admin/branding` as a new Administrator in `locale`.
async fn branding_as(h: &Harness, app: &TestApp, locale: &str) -> String {
    let (email, password) = (
        format!("admin-{locale}@sealbay.example"),
        "kelp forest at dawn",
    );
    let mut s = NewStaff::new(&email, "Admin", password, Locale::new(locale));
    s.roles = vec!["Administrator".into()];
    h.create_staff_with(s).await;
    let b = app.browser();
    assert_eq!(b.login(&email, password).await.status, 303);
    let page = b.get("/admin/branding").await;
    assert_eq!(page.status, 200);
    // One drawing per Menu image (locked, unlocked), in one Locale only.
    let figures = page.body.matches("<figure class=\"menu-preview\"").count();
    assert_eq!(figures, 2, "{locale}");
    page.body
}

#[tokio::test]
async fn the_branding_preview_labels_the_tap_areas_in_the_staff_members_locale() {
    let h = Harness::new();
    let app = boot(&h, &LineApi::default(), secrets(&[])).await;
    let th = branding_as(&h, &app, "th").await;
    assert!(th.contains(">ข่าวล่าสุด</text>"), "{th}");
    assert!(!th.contains("Latest news"), "{th}");
    let en = branding_as(&h, &app, "en").await;
    assert!(en.contains(">Latest news</text>"), "{en}");
    assert!(!en.contains(">ข่าวล่าสุด</text>"), "{en}");
    // A Locale with no texts (so no Menu of its own): the default Locale's.
    let ja = branding_as(&h, &app, "ja").await;
    assert!(ja.contains(">ข่าวล่าสุด</text>"), "{ja}");
    assert!(!ja.contains("Latest news"), "{ja}");
}
