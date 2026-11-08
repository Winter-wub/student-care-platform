//! Menu images replaced in the back office (#114) on recorded LINE HTTP.
//! A save checks the images, creates the new rich menus first (nothing
//! saved when LINE refuses), then the Kernel's re-link Jobs move every
//! account and retire the older salted revisions (ADR-0011); a reset
//! returns to the configured image the same way; a new locked image
//! re-locks every account that is not linked and retires the older
//! locked revisions. The page previews the tap areas.

mod common;

use std::sync::Arc;

use axum::body::Body;
use common::branding::with_locked;
use common::relink::*;
use common::{LineApi, png};
use harpseal::branding::{MenuImagesSaved, reset_menu_images, set_menu_images};
use harpseal::{MenuImageProblem, MenuSlot, RequestCtx, Statement, Value};
use harpseal_testing::{Harness, RecordedRequest};

const CONTENT: &str = "https://api-data.line.me/v2/bot/richmenu/";

/// A 2500 x 843 PNG header with `tag` after it: another image to hash.
fn image(tag: u8) -> Vec<u8> {
    let mut v = png(2500, 843);
    v.extend([tag; 16]);
    v
}

/// The image bytes uploaded to rich menu `id`.
fn uploaded(reqs: &[RecordedRequest], id: &str) -> Option<Vec<u8>> {
    let uri = format!("{CONTENT}{id}/content");
    reqs.iter()
        .find(|r| r.method == http::Method::POST && r.uri == uri)
        .map(|r| r.body.clone())
}

async fn count(h: &Harness, sql: &'static str) -> i64 {
    let rows = h.db().query(Statement::new(sql)).await.unwrap();
    rows.get(0, "n").and_then(Value::as_i64).unwrap_or(-1)
}

async fn save(s: &Setup, locked: Option<Vec<u8>>, unlocked: Option<Vec<u8>>) -> MenuImagesSaved {
    let d = s.app.deployment();
    set_menu_images(d, locked, unlocked, &s.editor, &RequestCtx::default())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_new_unlocked_image_is_staged_then_every_account_is_relinked_and_the_old_retired() {
    let s = setup(600).await;
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(0)).unwrap();
    s.api.take();

    assert_eq!(save(&s, None, Some(image(7))).await, MenuImagesSaved::Saved);
    // Staged before the save: the next revision of each Locale, on the new
    // image, created and uploaded; nobody moved yet.
    let reqs = s.api.requests();
    let names = s.api.live_names();
    let th2 = names
        .iter()
        .find(|n| n.starts_with("harpseal-th-") && is_rev(n, 2));
    let staged = s.api.id_of(th2.expect("revision 2 staged")).unwrap();
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("harpseal-en-") && is_rev(n, 1))
    );
    assert_eq!(uploaded(&reqs, &staged), Some(image(7)));
    assert_eq!(s.api.linked(&user(599)), Some(rev1.clone()));
    let audit = count(
        &s.h,
        "SELECT count(*) AS n FROM audit_log WHERE action = 'branding.menu_images'",
    )
    .await;
    assert_eq!(audit, 1);

    s.drain().await;
    let rev2 = s.api.linked(&user(0)).unwrap();
    assert_ne!(rev2, rev1);
    assert_eq!(uploaded(&s.api.requests(), &rev2), Some(image(7)));
    assert!(is_rev(&s.shown(0), 2), "{}", s.shown(0));
    for i in [1, 299, 599] {
        assert_eq!(s.api.linked(&user(i)), Some(rev2.clone()), "account {i}");
    }
    assert_eq!(s.api.batches().len(), 1, "the stragglers on revision 1");
    assert_eq!(s.deletes(), [rev1], "revision 1 retired, revision 0 kept");
    // Unchanged on a second save of the same image.
    assert_eq!(
        save(&s, None, Some(image(7))).await,
        MenuImagesSaved::Unchanged
    );
}

#[tokio::test]
async fn a_refused_create_or_upload_saves_nothing_and_every_account_keeps_its_menu() {
    let s = setup(3).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    let before = s.api.live_names();
    // The list answers, the create fails.
    s.api.http().respond_with(200, "{}");
    s.api
        .http()
        .respond_with(500, r#"{"message":"An error occurred in the backend"}"#);
    let d = s.app.deployment();
    let r = set_menu_images(d, None, Some(image(9)), &s.editor, &RequestCtx::default()).await;
    assert!(r.is_err(), "{r:?}");
    // The list and the create answer, the upload is refused: the new rich
    // menu is deleted again.
    s.api.http().respond_with(200, "{}");
    s.api.http().respond_with(200, "{}");
    s.api
        .http()
        .respond_with(400, r#"{"message":"Invalid image"}"#);
    let r = set_menu_images(d, None, Some(image(9)), &s.editor, &RequestCtx::default()).await;
    assert!(r.is_err(), "{r:?}");
    assert_eq!(s.api.live_names(), before, "nothing new left on LINE");
    assert_eq!(s.deletes().len(), 1, "the image-less rich menu went");
    for sql in [
        "SELECT count(*) AS n FROM setting WHERE key LIKE 'kernel.branding.%'",
        "SELECT count(*) AS n FROM audit_log WHERE action LIKE 'branding.%'",
        "SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.relink'",
        "SELECT count(*) AS n FROM menu_revision",
    ] {
        assert_eq!(count(&s.h, sql).await, 0, "{sql}");
    }
    assert!(s.h.files().keys().is_empty(), "the new file is gone");
    s.tick().await;
    assert_eq!(s.api.linked(&user(2)), Some(rev0));
}

#[tokio::test]
async fn images_are_checked_against_line_and_the_grid_first() {
    let s = setup(1).await;
    let d = s.app.deployment();
    let ctx = RequestCtx::default();
    let refused = |r: MenuImagesSaved| match r {
        MenuImagesSaved::Refused(p) => p,
        other => panic!("{other:?}"),
    };
    let p = refused(save(&s, None, Some(png(1200, 800))).await);
    assert!(matches!(
        p[0],
        (
            MenuSlot::Unlocked,
            MenuImageProblem::Size { width: 1200, .. }
        )
    ));
    // One row of tiles wants 2500 x 843.
    let p = refused(save(&s, None, Some(png(2500, 1686))).await);
    assert!(matches!(
        p[0].1,
        MenuImageProblem::Grid {
            rows: 1,
            want: (2500, 843)
        }
    ));
    let p = refused(save(&s, None, Some(b"GIF89a".to_vec())).await);
    assert_eq!(p[0].1, MenuImageProblem::NotImage);
    // This Deployment has no locked Menu: its image is never shown.
    let p = refused(save(&s, Some(image(1)), None).await);
    assert_eq!(p[0], (MenuSlot::Locked, MenuImageProblem::NoMenu));
    let r = reset_menu_images(d, &s.editor, &ctx).await.unwrap();
    assert_eq!(r, MenuImagesSaved::Unchanged, "nothing saved to reset");
    assert_eq!(s.api.batches().len() + s.deletes().len(), 0);
}

#[tokio::test]
async fn a_reset_returns_every_account_to_the_configured_image_the_same_way() {
    let s = setup(4).await;
    assert_eq!(save(&s, None, Some(image(3))).await, MenuImagesSaved::Saved);
    s.drain().await;
    let custom = s.api.linked(&user(0)).unwrap();
    assert_eq!(uploaded(&s.api.requests(), &custom), Some(image(3)));
    assert_eq!(s.h.files().keys().len(), 1);

    let d = s.app.deployment();
    let r = reset_menu_images(d, &s.editor, &RequestCtx::default()).await;
    assert_eq!(r.unwrap(), MenuImagesSaved::Saved);
    assert!(s.h.files().keys().is_empty(), "no setting names the file");
    s.drain().await;
    let back = s.api.linked(&user(3)).unwrap();
    assert_eq!(uploaded(&s.api.requests(), &back), Some(png(2500, 843)));
    assert!(is_rev(&s.shown(3), 2));
    assert!(
        s.deletes().contains(&custom),
        "the uploaded image's revision retired"
    );
    let n = count(
        &s.h,
        "SELECT count(*) AS n FROM audit_log WHERE action = 'branding.menu_images.reset'",
    )
    .await;
    assert_eq!(n, 1);
}

#[tokio::test]
async fn a_new_locked_image_relocks_every_account_not_linked_and_retires_older_revisions() {
    let api = LineApi::default();
    let (h, d) = with_locked(Harness::new(), Arc::new(api.clone()), 2, 25).await;
    let staff = harpseal::Actor::Staff(
        h.db()
            .query(Statement::new("SELECT id FROM staff"))
            .await
            .unwrap()
            .get(0, "id")
            .and_then(Value::as_text)
            .unwrap()
            .to_string(),
    );
    let ctx = RequestCtx::default();
    let locked0 = api.linked(&user(2)).unwrap();
    let tick = || async {
        for _ in 0..10 {
            h.clock().advance_minutes(61);
            d.tick(harpseal::Timestamp(h.clock().now_ms()))
                .await
                .unwrap();
        }
    };
    let r = set_menu_images(&d, Some(image(5)), None, &staff, &ctx).await;
    assert_eq!(r.unwrap(), MenuImagesSaved::Saved);
    tick().await;
    let locked1 = api.linked(&user(26)).unwrap();
    assert_ne!(locked1, locked0);
    let name = api.created(&locked1).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(name.starts_with("harpseal-locked-1-r1-"), "{name}");
    for i in 2..27 {
        assert_eq!(api.linked(&user(i)), Some(locked1.clone()), "account {i}");
    }
    assert_eq!(api.linked(&user(0)), None, "linked accounts untouched");
    assert!(api.batches().is_empty(), "revision 0 is never retired");

    let r = set_menu_images(&d, Some(image(6)), None, &staff, &ctx).await;
    assert_eq!(r.unwrap(), MenuImagesSaved::Saved);
    tick().await;
    let locked2 = api.linked(&user(2)).unwrap();
    assert_eq!(api.batches().len(), 1);
    assert_eq!(api.batches()[0].1, [(locked1.clone(), locked2.clone())]);
    let deletes: Vec<String> = api
        .requests()
        .iter()
        .filter(|r| r.method == http::Method::DELETE)
        .map(|r| r.uri.rsplit('/').next().unwrap().to_string())
        .collect();
    assert_eq!(deletes, [locked1], "only the older salted locked revision");
    let name0 = api.created(&locked0).unwrap()["name"].clone();
    let kept = api.id_of(name0.as_str().unwrap());
    assert_eq!(kept, Some(locked0), "the unsalted locked menu stays");
}

#[tokio::test]
async fn the_page_previews_the_tap_areas_and_applies_an_upload() {
    let s = setup(2).await;
    let b = s.app.browser();
    assert_eq!(
        b.login("editor@sealbay.example", "kelp forest at dawn")
            .await
            .status,
        303
    );
    let page = b.get("/admin/branding").await;
    assert_eq!(page.status, 200);
    assert!(page.body.contains("รูปเมนู LINE"));
    assert!(page.body.contains("href=\"/admin/branding/menu/unlocked\""));
    let current = b.get("/admin/branding/menu/unlocked").await;
    assert_eq!(current.header("content-type"), Some("image/png"));
    assert_eq!(current.header("x-content-type-options"), Some("nosniff"));

    let send = |op: &'static str, img: Vec<u8>| {
        let token = b.csrf().unwrap();
        let boundary = "----seal-menu";
        let mut body = Vec::new();
        for (name, value, file) in [
            ("_csrf", token.into_bytes(), false),
            ("op", op.as_bytes().to_vec(), false),
            ("unlocked", img, true),
        ] {
            let f = if file { "; filename=\"menu.png\"" } else { "" };
            let head = format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"{f}\r\n\r\n"
            );
            body.extend(head.into_bytes());
            body.extend(value);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{boundary}--\r\n").into_bytes());
        http::Request::post("/admin/branding/menu")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap()
    };
    let r = b.send(send("preview", png(2500, 1686))).await;
    assert_eq!(r.status, 422);
    assert!(
        r.body.contains("เมนูนี้มีปุ่ม 1 แถว รูปจึงต้องขนาด 2500 x 843 px"),
        "{}",
        r.body
    );
    let r = b.send(send("preview", image(4))).await;
    assert_eq!(r.status, 200);
    assert!(
        r.body
            .contains("<rect x=\"0\" y=\"0\" width=\"2500\" height=\"843\">"),
        "{}",
        r.body
    );
    assert!(r.body.contains("name=\"unlocked_draft\""));
    assert_eq!(
        count(&s.h, "SELECT count(*) AS n FROM setting").await,
        0,
        "preview saves nothing"
    );
    let r = b.send(send("apply", image(4))).await;
    assert_eq!(r.status, 303, "{}", r.body);
    s.drain().await;
    let shown = s.api.linked(&user(1)).unwrap();
    assert_eq!(uploaded(&s.api.requests(), &shown), Some(image(4)));
}
