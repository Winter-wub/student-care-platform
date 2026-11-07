//! Kernel migrations on an in-memory SQLite `Db`: every table from the data
//! model exists, tracking makes a second run a no-op, and the ADR-0007 rules
//! the schema can enforce are enforced.

use harpseal::{Statement, Value};
use harpseal_testing::Harness;

const KERNEL_TABLES: &[&str] = &[
    "student",
    "tag",
    "student_tag",
    "channel_account",
    "link",
    "verification_challenge",
    "consent",
    "security_event",
    "inbound_message",
    "staff",
    "role",
    "role_permission",
    "staff_role",
    "staff_session",
    "app_session",
    "rate_limit",
    "outbound_message",
    "usage_daily",
    "job",
    "audit_log",
    "setting",
    "roster_import",
    "migration",
];

async fn exec(h: &Harness, sql: &'static str) -> harpseal::Result<u64> {
    h.db()
        .execute(Statement::new(sql))
        .await
        .map(|r| r.rows_affected)
}

#[tokio::test]
async fn every_kernel_table_exists_after_migrate() {
    let h = Harness::new();
    let app = h.start().await;
    let rows = app
        .query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .await;
    let names: Vec<&str> = rows.rows.iter().filter_map(|r| r[0].as_text()).collect();
    for t in KERNEL_TABLES {
        assert!(names.contains(t), "missing table {t}; have {names:?}");
    }
}

#[tokio::test]
async fn migrations_apply_twice_without_error_and_are_tracked() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let first = d.migrate().await.unwrap();
    assert_eq!(first.applied.len(), d.migrations().len());
    assert_eq!(first.skipped, 0);
    let second = d.migrate().await.unwrap();
    assert!(second.applied.is_empty());
    assert_eq!(second.skipped, d.migrations().len());

    let rows = h
        .db()
        .query(Statement::new(
            "SELECT module, name, applied_at, checksum FROM migration ORDER BY name",
        ))
        .await
        .unwrap();
    let recorded: Vec<(String, String)> = rows
        .rows
        .iter()
        .map(|r| {
            (
                r[0].as_text().unwrap().into(),
                r[1].as_text().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        recorded,
        vec![
            ("kernel".into(), "0001_people".into()),
            ("kernel".into(), "0002_channels".into()),
            ("kernel".into(), "0003_operations".into()),
            ("kernel".into(), "0004_inbound_file_error".into()),
            ("kernel".into(), "0005_job_unique_key".into()),
            ("kernel".into(), "0006_inbound_event_id".into()),
            ("kernel".into(), "0007_rate_limit".into()),
            ("kernel".into(), "0008_app_session_expires_idx".into()),
        ]
    );
    assert_eq!(rows.rows[0][2], Value::Integer(harpseal_testing::START_MS));
    assert_eq!(
        rows.rows[0][3].as_text().map(str::len),
        Some(64),
        "SHA-256 hex"
    );
}

#[tokio::test]
async fn a_module_migration_runs_after_the_kernel_and_once() {
    struct Notes;
    impl harpseal::Module for Notes {
        fn name(&self) -> &'static str {
            "notes"
        }
        fn register(&self, r: &mut harpseal::ModuleRegistrar) {
            r.migration(
                "0001_notes",
                "CREATE TABLE note (id TEXT PRIMARY KEY, \
                 student_id TEXT NOT NULL REFERENCES student (id));",
            );
        }
    }
    let h = Harness::new();
    let d = h.builder().module(Notes).build().unwrap();
    let names: Vec<&str> = d.migrations().iter().map(|m| m.module).collect();
    assert_eq!(
        names,
        [
            "kernel", "kernel", "kernel", "kernel", "kernel", "kernel", "kernel", "kernel", "notes"
        ]
    );
    assert_eq!(d.migrate().await.unwrap().applied.len(), 9);
    assert!(d.migrate().await.unwrap().applied.is_empty());
}

#[tokio::test]
async fn a_failing_migration_leaves_nothing_behind() {
    struct Broken;
    impl harpseal::Module for Broken {
        fn name(&self) -> &'static str {
            "broken"
        }
        fn register(&self, r: &mut harpseal::ModuleRegistrar) {
            r.migration(
                "0001_broken",
                "CREATE TABLE half (x TEXT); CREATE TABLE oops (;",
            );
        }
    }
    let h = Harness::new();
    let d = h.builder().module(Broken).build().unwrap();
    assert!(d.migrate().await.is_err());
    let rows = h
        .db()
        .query(Statement::new(
            "SELECT count(*) FROM sqlite_master WHERE name = 'half'",
        ))
        .await
        .unwrap();
    assert_eq!(rows.rows[0][0], Value::Integer(0), "batch rolled back");
}

#[tokio::test]
async fn audit_log_is_append_only() {
    let h = Harness::new();
    let _app = h.start().await;
    exec(
        &h,
        "INSERT INTO audit_log (id, at, actor_kind, action, before, after) \
         VALUES ('019a5000-0000-7000-8000-000000000001', 1, 'system', 'test', NULL, '{\"a\":1}')",
    )
    .await
    .unwrap();
    let update = exec(&h, "UPDATE audit_log SET action = 'changed'").await;
    assert!(update.is_err(), "update must be refused");
    let delete = exec(&h, "DELETE FROM audit_log").await;
    assert!(delete.is_err(), "delete must be refused");
}

#[tokio::test]
async fn one_live_link_per_student_per_channel() {
    let h = Harness::new();
    let _app = h.start().await;
    let setup = [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010001', 'ปลายฟ้า คลื่นเงิน', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1), ('a2', 'fake', 'u2', 1, 1)",
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
         VALUES ('l1', 's1', 'a1', 'fake', 'active', 'email_otp', 1, 1)",
    ];
    for sql in setup {
        exec(&h, sql).await.unwrap();
    }
    let second = "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, linked_at, updated_at) \
                  VALUES ('l2', 's1', 'a2', 'fake', 'active', 'email_otp', 2, 2)";
    assert!(exec(&h, second).await.is_err());
    exec(
        &h,
        "UPDATE link SET status = 'revoked', revoked_at = 2 WHERE id = 'l1'",
    )
    .await
    .unwrap();
    exec(&h, second)
        .await
        .expect("allowed once the first is revoked");
}

#[tokio::test]
async fn consent_rows_are_never_deleted() {
    let h = Harness::new();
    let _app = h.start().await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010001', 'น้ำใส ดาวทะเล', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1)",
        "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, accepted_at) \
         VALUES ('c1', 's1', 'a1', '1', 'th', 1)",
    ] {
        exec(&h, sql).await.unwrap();
    }
    assert!(exec(&h, "DELETE FROM consent").await.is_err());
}

struct Versioned(&'static str);

impl harpseal::Module for Versioned {
    fn name(&self) -> &'static str {
        "versioned"
    }
    fn register(&self, r: &mut harpseal::ModuleRegistrar) {
        r.migration("0001_thing", self.0);
    }
}

#[tokio::test]
async fn an_applied_migration_that_changed_is_refused() {
    let h = Harness::new();
    let v1 = h
        .builder()
        .module(Versioned("CREATE TABLE thing (id TEXT PRIMARY KEY);"));
    v1.build().unwrap().migrate().await.unwrap();
    let v2 = h
        .builder()
        .module(Versioned(
            "CREATE TABLE thing (id TEXT PRIMARY KEY, x TEXT);",
        ))
        .build()
        .unwrap();
    let err = v2.migrate().await.unwrap_err().to_string();
    assert!(
        err.contains("versioned/0001_thing changed after it was applied"),
        "{err}"
    );
    let same = h
        .builder()
        .module(Versioned("CREATE TABLE thing (id TEXT PRIMARY KEY);"))
        .build()
        .unwrap();
    assert!(same.migrate().await.unwrap().applied.is_empty());
}

#[tokio::test]
async fn consent_is_once_per_notice_version_and_never_changed() {
    let h = Harness::new();
    let _app = h.start().await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010001', 'น้ำใส ดาวทะเล', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1)",
        "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, accepted_at) \
         VALUES ('c1', 's1', 'a1', '1', 'th', 1)",
    ] {
        exec(&h, sql).await.unwrap();
    }
    let again = "INSERT INTO consent (id, student_id, channel_account_id, notice_version, locale, accepted_at) \
                 VALUES ('c2', 's1', 'a1', '1', 'th', 2)";
    assert!(exec(&h, again).await.is_err(), "one row per version");
    assert!(exec(&h, "UPDATE consent SET locale = 'en'").await.is_err());
}

#[tokio::test]
async fn revoked_by_and_added_by_must_name_staff() {
    let h = Harness::new();
    let _app = h.start().await;
    for sql in [
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         VALUES ('s1', '68010001', 'ปลายฟ้า คลื่นเงิน', 'th', 1, 1)",
        "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         VALUES ('a1', 'fake', 'u1', 1, 1)",
        "INSERT INTO tag (id, name, created_at) VALUES ('t1', 'ปี 1', 1)",
    ] {
        exec(&h, sql).await.unwrap();
    }
    let link = "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                linked_at, revoked_at, revoked_by, updated_at) \
                VALUES ('l1', 's1', 'a1', 'fake', 'revoked', 'manual', 1, 2, 'nobody', 2)";
    assert!(exec(&h, link).await.is_err());
    let tag = "INSERT INTO student_tag (student_id, tag_id, source, added_at, added_by) \
               VALUES ('s1', 't1', 'manual', 1, 'nobody')";
    assert!(exec(&h, tag).await.is_err());
}
