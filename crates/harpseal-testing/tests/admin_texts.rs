//! Texts page (#27) over HTTP and end to end: an edited text is what the
//! Fake Channel receives next, placeholders are checked, reset to default,
//! and a Consent version bump asks linked Students again.

mod admin_support;
mod support;

use admin_support::{audits, boot, flash, hidden, htmx_get, signed_in, text};
use harpseal::Message;
use harpseal_testing::urlencode;
use support::{Chat, text_of};

const WELCOME: &str = "/admin/texts/kernel.flow.welcome";
const DEFAULT_WELCOME: &str = "สวัสดีครับ ยินดีต้อนรับสู่ช่องทางบริการนักศึกษา";

/// The form of `locale`, as a browser posts it: `version` is the token
/// the page showed (`-` when no `setting` row existed).
fn save(locale: &str, value: &str, version: &str) -> String {
    format!(
        "locale={locale}&value={}&version={}&op=save",
        urlencode(value),
        urlencode(version)
    )
}

/// The `version` token of the `locale` form on the edit page at `path`.
async fn token(b: &harpseal_testing::Browser<'_>, path: &str, locale: &str) -> String {
    let body = b.get(path).await.body;
    let at = body
        .find(&format!("name=\"locale\" value=\"{locale}\""))
        .expect("the Locale's form");
    hidden(&body[at..], "version")
}

#[tokio::test]
async fn an_edited_welcome_is_what_the_next_follower_receives_until_reset() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let new = "ยินดีต้อนรับสู่ Seal Bay! อ่านประกาศด้านล่างก่อนนะ";
    let r = admin.post(WELCOME, &save("th", new, "-")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    assert_eq!(audits(&h, "setting.update").await, 1);
    let before = text(
        &h,
        "SELECT before FROM audit_log WHERE action = ?",
        "setting.update",
        "before",
    )
    .await
    .unwrap();
    assert!(
        before.contains(DEFAULT_WELCOME),
        "before = the old effective text"
    );

    let mut c = Chat::new(&h, &app, "newcomer");
    let seen = c.follow().await;
    assert_eq!(text_of(&seen[0]), new);

    // The page shows the source; a stale form is refused.
    let page = admin.get(WELCOME).await.body;
    assert!(page.contains("แก้ในหลังบ้าน"));
    admin.post(WELCOME, &save("th", "อีกแบบ", "-")).await;
    assert_eq!(flash(&admin), "err:kernel.admin.conflict");

    let v = token(&admin, WELCOME, "th").await;
    let reset = format!("locale=th&value=&version={v}&op=reset");
    admin.post(WELCOME, &reset).await;
    assert_eq!(flash(&admin), "ok:kernel.admin.texts.reset_done");
    assert_eq!(app.count("SELECT count(*) FROM setting").await, 0);
    assert_eq!(audits(&h, "setting.reset").await, 1);
    let mut d = Chat::new(&h, &app, "second");
    assert!(text_of(&d.follow().await[0]).starts_with(DEFAULT_WELCOME));
}

#[tokio::test]
async fn placeholders_must_match_the_default() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let path = "/admin/texts/kernel.inbound.file_too_large";
    let page = admin.get(path).await;
    assert!(
        page.body.contains("<code>{mb}</code>"),
        "placeholders listed"
    );

    let dropped = admin.post(path, &save("th", "ไฟล์ใหญ่เกินไป", "-")).await;
    assert_eq!(dropped.status, 422);
    assert!(dropped.body.contains("ข้อความขาดตัวแทนค่า"));
    assert!(
        dropped.body.contains("ไฟล์ใหญ่เกินไป"),
        "the value is kept in the form"
    );
    let added = admin
        .post(path, &save("en", "Over {mb} MB ({gb} GB)", "-"))
        .await;
    assert_eq!(added.status, 422);
    assert!(added.body.contains("<code>{gb}</code>"));
    assert_eq!(admin.post(path, &save("th", "  ", "-")).await.status, 422);
    assert_eq!(app.count("SELECT count(*) FROM setting").await, 0);

    let ok = admin
        .post(path, &save("en", "Files over {mb} MB are not kept.", "-"))
        .await;
    assert_eq!(ok.status, 303);
    assert_eq!(admin.get("/admin/texts/kernel.nope").await.status, 404);
    assert_eq!(admin.post(path, &save("xx", "{mb}", "-")).await.status, 404);
}

#[tokio::test]
async fn the_list_shows_every_key_with_its_source_and_searches() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    admin.post(WELCOME, &save("en", "Hi there", "-")).await;
    let list = admin.get("/admin/texts").await;
    assert_eq!(list.status, 200);
    for key in [
        "kernel.flow.welcome",
        "kernel.notice.body",
        "kernel.link.ask_student_id",
    ] {
        assert!(
            list.body.contains(&format!("href=\"/admin/texts/{key}\"")),
            "{key}"
        );
    }
    // Back-office page texts are read at build time: not offered.
    for key in [
        "kernel.admin.texts.title",
        "kernel.students.title",
        "kernel.auth.sign_in",
    ] {
        assert!(
            !list.body.contains(&format!("/admin/texts/{key}\"")),
            "{key}"
        );
        assert_eq!(admin.get(&format!("/admin/texts/{key}")).await.status, 404);
    }
    let r = admin
        .post("/admin/texts/kernel.students.title", &save("th", "x", "-"))
        .await;
    assert_eq!(r.status, 404);
    // A history restore gets the whole page.
    let req = http::Request::get("/admin/texts?q=welcome")
        .header("hx-request", "true")
        .header("hx-history-restore-request", "true")
        .body(axum::body::Body::empty())
        .unwrap();
    let restored = admin.send(req).await;
    assert!(restored.body.contains("<html"), "full page");
    assert_eq!(restored.header("vary"), Some("HX-Request"));
    assert!(list.body.contains("deployment.toml"), "TOML source");
    assert!(list.body.contains("edited here") || list.body.contains("แก้ในหลังบ้าน"));
    let found = htmx_get(&admin, "/admin/texts?q=Hi+there").await;
    assert!(found.body.starts_with("<div id=\"texts-results\""));
    assert!(found.body.contains("kernel.flow.welcome"));
    assert!(!found.body.contains("kernel.menu.pick"));
    assert_eq!(found.header("vary"), Some("HX-Request"));
    let by_key = admin.get("/admin/texts?q=menu.pick").await;
    assert!(
        by_key.body.contains("เลือกบริการจากเมนูด้านล่างได้เลย"),
        "TOML text shown"
    );
}

#[tokio::test]
async fn bumping_the_consent_version_asks_a_linked_student_again() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "linked");
    let done = c.link("68010001").await;
    assert!(text_of(&done[0]).starts_with("เชื่อมต่อสำเร็จ"), "{done:?}");
    assert_eq!(
        app.count("SELECT count(*) FROM consent WHERE notice_version = '1'")
            .await,
        1
    );
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    assert!(
        admin
            .get("/admin/texts")
            .await
            .body
            .contains("<strong class=\"code\">1</strong>")
    );
    let same = admin
        .post("/admin/texts/consent", "version=1&expected=-")
        .await;
    assert_eq!(same.status, 303);
    assert_eq!(flash(&admin), "err:kernel.admin.texts.consent_same");
    admin
        .post("/admin/texts/consent", "version=2+0&expected=-")
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.texts.consent_invalid");
    admin
        .post("/admin/texts/consent", "version=2&expected=-")
        .await;
    assert_eq!(flash(&admin), "ok:kernel.admin.texts.consent_saved");
    let stored = text(
        &h,
        "SELECT value FROM setting WHERE key = ?",
        "kernel.consent.version",
        "value",
    )
    .await;
    assert_eq!(stored.as_deref(), Some("2"));

    // The next Inbound gets the updated Consent Card.
    let seen = c.text("สวัสดี").await;
    let card = seen.iter().find_map(|m| match m {
        Message::Card(card) => Some(card.clone()),
        _ => None,
    });
    assert_eq!(
        card.expect("a Consent Card").title,
        "ประกาศความเป็นส่วนตัวฉบับใหม่"
    );
    c.press("kernel.consent.accept").await;
    assert_eq!(
        app.count("SELECT count(*) FROM consent WHERE notice_version = '2'")
            .await,
        1
    );
    // A stale form (it still thinks there is no row) is refused.
    admin
        .post("/admin/texts/consent", "version=3&expected=-")
        .await;
    assert_eq!(flash(&admin), "err:kernel.admin.conflict");
}

#[tokio::test]
async fn a_multi_line_text_is_saved_twice_and_reset_from_a_browser_and_notice_shows_it() {
    let (h, app) = boot().await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let path = "/admin/texts/kernel.notice.body";
    // Browsers send CRLF in every field.
    let first = "เราเก็บรหัสนักศึกษาเท่านั้น\r\n\r\nติดต่อเจ้าหน้าที่ได้ที่ห้อง 101";
    let v = token(&admin, path, "th").await;
    assert_eq!(v, "-");
    admin.post(path, &save("th", first, &v)).await;
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");
    let notice = app.get("/notice").await;
    assert!(
        notice.body.contains("<p>ติดต่อเจ้าหน้าที่ได้ที่ห้อง 101</p>"),
        "{}",
        notice.body
    );

    let second = "ฉบับแก้ไข\r\n\r\nติดต่อห้อง 202";
    let v = token(&admin, path, "th").await;
    admin.post(path, &save("th", second, &v)).await;
    assert_eq!(
        flash(&admin),
        "ok:kernel.admin.saved",
        "a second save is not a conflict"
    );
    let stored = text(
        &h,
        "SELECT value FROM setting WHERE key = ?",
        "kernel.notice.body",
        "value",
    )
    .await
    .unwrap();
    assert_eq!(stored, "ฉบับแก้ไข\n\nติดต่อห้อง 202");

    let v = token(&admin, path, "th").await;
    let reset = format!("locale=th&value={}&version={v}&op=reset", urlencode(second));
    admin.post(path, &reset).await;
    assert_eq!(flash(&admin), "ok:kernel.admin.texts.reset_done");
    assert_eq!(app.count("SELECT count(*) FROM setting").await, 0);
    assert!(!app.get("/notice").await.body.contains("ห้อง 202"));
}

#[tokio::test]
async fn an_edited_menu_label_keeps_the_menu_key() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "linked");
    c.link("68010001").await;
    let before = c.menu().flatten().expect("Menu unlocked");
    assert_eq!(before.key(), Some("th"));

    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let path = "/admin/texts/kernel.menu.unlink";
    let edited = "เลิกเชื่อมต่อบัญชี";
    let r = admin.post(path, &save("th", edited, "-")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&admin), "ok:kernel.admin.saved");

    // A follow sets the Menu again, now with the edited label.
    c.follow().await;
    let after = c.menu().flatten().expect("Menu unlocked");
    let labels = |m: &harpseal::Menu| {
        m.entries
            .iter()
            .map(|e| e.label.clone())
            .collect::<Vec<_>>()
    };
    assert!(labels(&after).contains(&edited.to_string()), "{after:?}");
    assert_ne!(labels(&after), labels(&before));
    assert_eq!(after.key(), before.key(), "same Menu key after the edit");
}
