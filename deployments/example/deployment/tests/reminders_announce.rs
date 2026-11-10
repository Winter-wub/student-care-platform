//! A Module page calls a Core service (#106): the reminder page drafts an
//! Announcement to pasted Students through
//! `modules::announcements::create_draft` as the signed-in Staff member,
//! with the Core page's Permission check and audit row.

// axum at the Core's version (#162): this crate has no axum dependency.
use harpseal::web::axum;

use axum::body::Body;
use axum::http::{Request, header};
use example_deployment::{Secrets, builder, reminders};
use harpseal::{Actor, RequestCtx, Statement, Value};
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

const PASSWORD: &str = "kelp forest at dawn";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

/// Karn, Mali and Niran (fictional), on the roster.
async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    for (id, code, name) in [
        ("s1", "68010001", "Karn Seal"),
        ("s2", "68010002", "Mali Tern"),
        ("s3", "68010003", "Niran Gull"),
    ] {
        let sql = format!(
            "INSERT INTO student (id, student_code, display_name, locale, created_at, \
             updated_at) VALUES ('{id}', '{code}', '{name}', 'th', 1, 1)"
        );
        h.db().execute(Statement::new(sql)).await.unwrap();
    }
    app
}

async fn signed_in<'a>(app: &'a TestApp, email: &str) -> Browser<'a> {
    let b = app.browser();
    assert_eq!(b.login(email, PASSWORD).await.status, 303);
    // The form page leaves the CSRF token in the browser.
    b.get("/admin/reminders").await;
    b
}

/// `POST path` with the CSRF token and a User-Agent, as a browser would.
async fn post(b: &Browser<'_>, path: &str, form: &str) -> TestResponse {
    let form = format!("{form}&_csrf={}", b.csrf().unwrap_or_default());
    let req = Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::USER_AGENT, "SealBayTest/1.0")
        .body(Body::from(form))
        .unwrap();
    b.send(req).await
}

fn text(rows: &harpseal::Rows, col: &str) -> String {
    rows.get(0, col)
        .and_then(Value::as_text)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn the_reminder_page_drafts_an_announcement_to_the_pasted_students_as_that_staff() {
    let h = Harness::new();
    let app = boot(&h).await;
    let officer = h
        .create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    let b = signed_in(&app, "officer@sealbay.example").await;
    let form = b.get("/admin/reminders").await.body;
    assert!(form.contains("formaction=\"/admin/reminders/announce\""));

    let r = post(&b, "/admin/reminders/announce", "ids=68010003%0A68010001").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let to = r.header("location").unwrap().to_string();
    let id = to
        .strip_prefix("/admin/announcements/")
        .unwrap()
        .to_string();
    assert_eq!(b.get(&to).await.status, 200, "the Core detail page opens");

    let row = app
        .query("SELECT id, status, audience, require_ack, created_by FROM announcement")
        .await;
    assert_eq!(row.len(), 1);
    assert_eq!(text(&row, "id"), id);
    assert_eq!(text(&row, "status"), "draft");
    assert_eq!(text(&row, "created_by"), officer);
    assert_eq!(row.get(0, "require_ack").and_then(Value::as_i64), Some(1));
    let audience: serde_json::Value = serde_json::from_str(&text(&row, "audience")).unwrap();
    assert_eq!(
        audience["students"],
        serde_json::json!(["68010003", "68010001"])
    );
    assert_eq!(audience["tags"], serde_json::json!([]));
    assert_eq!(audience["attributes"], serde_json::json!({}));
    let texts = app
        .query("SELECT locale, title FROM announcement_text ORDER BY locale")
        .await;
    assert_eq!(texts.len(), 2);
    assert_eq!(text(&texts, "title"), "Fee reminder");

    let audit = app
        .query(
            "SELECT actor_kind, actor_id, target_id, user_agent, request_id FROM audit_log \
             WHERE action = 'announcement.create'",
        )
        .await;
    assert_eq!(audit.len(), 1);
    assert_eq!(text(&audit, "actor_kind"), "staff");
    assert_eq!(text(&audit, "actor_id"), officer);
    assert_eq!(text(&audit, "target_id"), id);
    assert_eq!(text(&audit, "user_agent"), "SealBayTest/1.0");
    assert_eq!(
        text(&audit, "request_id"),
        r.header("x-request-id").unwrap(),
        "the request's own RequestCtx"
    );

    // An empty list would be everyone: refused, nothing drafted.
    let r = post(&b, "/admin/reminders/announce", "ids=").await;
    assert_eq!(r.status, 422);
    assert_eq!(app.count("SELECT count(*) FROM announcement").await, 1);

    // More than the Core's hand-picked limit: refused before the Audience.
    let over: Vec<String> = (0..=harpseal::audience::MAX_STUDENTS)
        .map(|n| format!("{:08}", 68_020_000 + n))
        .collect();
    let r = post(
        &b,
        "/admin/reminders/announce",
        &format!("ids={}", over.join("%0A")),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.body);
    assert!(r.body.contains("2,000"), "{}", r.body);
    assert_eq!(app.count("SELECT count(*) FROM announcement").await, 1);
}

#[tokio::test]
async fn staff_without_announcements_send_get_the_core_pages_refusal() {
    let h = Harness::new();
    let app = boot(&h).await;
    // A Clerk may send the fee reminder but not announcements.
    let made = harpseal::ensure_role(
        &h.services(),
        &Actor::System,
        "Clerk",
        &[reminders::PERMISSION.id],
        &RequestCtx::cli(),
    )
    .await
    .unwrap();
    assert!(made);
    h.create_staff("clerk@sealbay.example", PASSWORD, &["Clerk"])
        .await;
    let b = signed_in(&app, "clerk@sealbay.example").await;

    let ours = post(&b, "/admin/reminders/announce", "ids=68010001").await;
    let core = post(&b, "/admin/announcements/new", "title_th=x&body_th=y").await;
    assert_eq!(ours.status, 403);
    assert_eq!(core.status, 403);
    assert!(ours.body.contains("announcements.send"), "{}", ours.body);
    // The same refusal as the Core's; the shell around it marks the page
    // the request came from (#123), so compare the main column.
    let main = |b: &str| b.split("<main").nth(1).map(str::to_string);
    assert!(main(&ours.body).is_some());
    assert_eq!(
        main(&ours.body),
        main(&core.body),
        "the same 403 page as the Core's"
    );

    assert_eq!(app.count("SELECT count(*) FROM announcement").await, 0);
    let audited = "SELECT count(*) FROM audit_log WHERE action LIKE 'announcement.%'";
    assert_eq!(app.count(audited).await, 0);
}
