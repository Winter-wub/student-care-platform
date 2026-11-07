//! The Backups page (#78) over HTTP: the `kernel.backup.manage`
//! Permission, the list, "back up now", one backup as a `.tar` and all
//! data as an `.xlsx` without secret columns, each download audited.

mod backup_support;

use std::io::Cursor;

use backup_support::nightly::{NIGHT, admin, night, stamp_of, ticks, untar};
use backup_support::{OFFICER, PASSWORD, populated, query, text};
use calamine::{Data, Reader, Xlsx};
use harpseal::FileStore;
use harpseal_testing::Harness;

async fn audits(h: &Harness, action: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM audit_log WHERE action = '{action}'");
    query(h.db().as_ref(), &sql).await.rows[0][0]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn only_holders_of_the_permission_reach_the_page_and_downloads() {
    let (h, app) = populated().await;
    night(&h, &app, 0).await;
    let id = text(&h, "SELECT id FROM backup_run").await;

    // The Officer holds no `kernel.backup.manage`.
    let officer = app.browser();
    assert_eq!(officer.login(OFFICER, PASSWORD).await.status, 303);
    for path in [
        "/admin/backups".to_string(),
        "/admin/backups/export.xlsx".to_string(),
        format!("/admin/backups/{id}/download"),
    ] {
        assert_eq!(officer.get(&path).await.status, 403, "{path}");
    }
    assert_eq!(officer.post("/admin/backups/now", "").await.status, 403);
    let home = officer.get("/admin/students").await;
    assert!(
        !home.body.contains("href=\"/admin/backups\""),
        "not in the nav"
    );
    assert_eq!(audits(&h, "backup.download").await, 0);
    assert_eq!(audits(&h, "data.export").await, 0);

    let b = admin(&h, &app).await;
    let page = b.get("/admin/backups").await;
    assert_eq!(page.status, 200, "{}", page.body);
    assert!(page.body.contains("href=\"/admin/backups\""), "in the nav");
    assert!(
        page.body
            .contains(&format!("data-id=\"{id}\" data-state=\"done\""))
    );
    assert!(page.body.contains(&format!("/admin/backups/{id}/download")));
    assert!(page.body.contains("0 2 * * *"), "the schedule is shown");
    assert_eq!(b.get("/admin/backups/nope/download").await.status, 404);
}

#[tokio::test]
async fn a_backup_downloads_as_a_tar_of_its_objects() {
    let (h, app) = populated().await;
    night(&h, &app, 0).await;
    let id = text(&h, "SELECT id FROM backup_run").await;
    let b = admin(&h, &app).await;

    let r = b.get(&format!("/admin/backups/{id}/download")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("content-type"), Some("application/x-tar"));
    let dir = stamp_of(NIGHT);
    assert_eq!(
        r.header("content-disposition").unwrap(),
        format!("attachment; filename=\"backup-{dir}.tar\"")
    );
    let entries = untar(&r.bytes);
    assert_eq!(entries[0].0, format!("{dir}/manifest.json"));
    let prefix = format!("backups/{dir}/");
    let stored: Vec<String> = h
        .files()
        .keys()
        .into_iter()
        .filter(|k| k.starts_with(&prefix))
        .collect();
    assert_eq!(entries.len(), stored.len(), "every object, once");
    for (path, bytes) in &entries {
        let key = format!("backups/{path}");
        let object = h.files().get(&key).await.unwrap().unwrap();
        assert_eq!(&object, bytes, "{path}");
    }
    assert_eq!(audits(&h, "backup.download").await, 1);
    let target = text(
        &h,
        "SELECT target_id FROM audit_log WHERE action = 'backup.download'",
    )
    .await;
    assert_eq!(target, id);
}

#[tokio::test]
async fn all_data_downloads_as_xlsx_without_secrets() {
    let (h, app) = populated().await;
    let b = admin(&h, &app).await;
    let r = b.get("/admin/backups/export.xlsx").await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.header("content-type"),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
    );
    let mut book: Xlsx<_> = Xlsx::new(Cursor::new(r.bytes.clone())).unwrap();
    let sheets = book.sheet_names();
    for t in [
        "student",
        "staff",
        "staff_session",
        "audit_log",
        "announcement",
    ] {
        assert!(sheets.iter().any(|s| s == t), "{t} in {sheets:?}");
    }
    assert!(!sheets.iter().any(|s| s == "migration"), "as the backup");
    let header = |book: &mut Xlsx<_>, sheet: &str| -> Vec<String> {
        let range = book.worksheet_range(sheet).unwrap();
        range
            .rows()
            .next()
            .unwrap()
            .iter()
            .map(|c| c.to_string())
            .collect()
    };
    let staff = header(&mut book, "staff");
    assert!(staff.contains(&"email".to_string()), "{staff:?}");
    assert!(
        !staff.iter().any(|c| c.contains("password_hash")),
        "{staff:?}"
    );
    let sessions = header(&mut book, "staff_session");
    assert!(!sessions.contains(&"id".to_string()), "{sessions:?}");
    assert!(sessions.contains(&"staff_id".to_string()));
    for (sheet, gone) in [
        ("verification_challenge", "secret_hash"),
        ("job", "lock_token"),
    ] {
        assert!(
            !header(&mut book, sheet).contains(&gone.to_string()),
            "{sheet}"
        );
    }
    // The rows came along: three Students, with their codes.
    let students = book.worksheet_range("student").unwrap();
    assert_eq!(students.height(), 4, "header and three rows");
    let codes: Vec<&Data> = students.rows().skip(1).flat_map(|r| r.iter()).collect();
    assert!(
        codes
            .iter()
            .any(|c| matches!(c, Data::String(s) if s == "68010002"))
    );

    assert_eq!(audits(&h, "data.export").await, 1);
    let after = text(
        &h,
        "SELECT after FROM audit_log WHERE action = 'data.export'",
    )
    .await;
    let after: serde_json::Value = serde_json::from_str(&after).unwrap();
    let excluded = after["excluded_columns"].to_string();
    assert!(excluded.contains("staff.password_hash"), "{excluded}");
    assert!(excluded.contains("staff_session.id"), "{excluded}");
}

#[tokio::test]
async fn back_up_now_starts_a_manual_run_once() {
    let (h, app) = populated().await;
    let b = admin(&h, &app).await;
    assert_eq!(b.get("/admin/backups").await.status, 200, "a CSRF token");
    let r = b.post("/admin/backups/now", "").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(b.post("/admin/backups/now", "").await.status, 303);
    assert_eq!(audits(&h, "backup.request").await, 2);
    ticks(&h, &app, 1).await;
    // Running: the button is disabled and a press is refused.
    let page = b.get("/admin/backups").await;
    assert!(
        page.body.contains("data-state=\"running\""),
        "{}",
        page.body
    );
    assert_eq!(b.post("/admin/backups/now", "").await.status, 303);
    assert_eq!(
        audits(&h, "backup.request").await,
        2,
        "refused while running"
    );
    ticks(&h, &app, 5).await;
    let rows = query(h.db().as_ref(), "SELECT state, origin FROM backup_run").await;
    assert_eq!(rows.len(), 1, "one run for two presses in one minute");
    assert_eq!(rows.rows[0][0].as_text(), Some("done"));
    assert_eq!(rows.rows[0][1].as_text(), Some("manual"));
}
