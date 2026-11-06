//! Native port tests: Db, FileStore (local, S3 when configured), Clock,
//! Random, settings.

use std::sync::Arc;

use harpseal::{Clock, Db, FileStore, Random, Statement, Value};
use harpseal_runtime_native::{
    EmailSettings, FilesSettings, LocalFiles, NativeSettings, OsRandom, S3Files, S3Settings,
    SqliteDb, SystemClock,
};

fn scratch(name: &str) -> std::path::PathBuf {
    let mut b = [0u8; 8];
    OsRandom.fill(&mut b).unwrap();
    let tag: String = b.iter().map(|x| format!("{x:02x}")).collect();
    std::env::temp_dir().join(format!("harpseal-{name}-{tag}"))
}

async fn notes_db() -> SqliteDb {
    let db = SqliteDb::memory().unwrap();
    db.execute(Statement::new(
        "CREATE TABLE note (id TEXT PRIMARY KEY, body TEXT NOT NULL, n INTEGER, r REAL, b BLOB)",
    ))
    .await
    .unwrap();
    db
}

#[tokio::test]
async fn db_batch_rolls_back_when_one_statement_fails() {
    let db = notes_db().await;
    let insert = |id: &str, body: Option<&str>| {
        Statement::new("INSERT INTO note (id, body) VALUES (?, ?)")
            .bind(id)
            .bind(body)
    };
    let err = db
        .batch(vec![
            insert("a", Some("first")),
            insert("b", Some("second")),
            insert("c", None), // NOT NULL violation
        ])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("NOT NULL"), "{err}");
    let rows = db
        .query(Statement::new("SELECT count(*) AS n FROM note"))
        .await
        .unwrap();
    assert_eq!(rows.get(0, "n").and_then(Value::as_i64), Some(0));

    let ok = db
        .batch(vec![
            insert("a", Some("first")),
            insert("b", Some("second")),
        ])
        .await
        .unwrap();
    assert_eq!(ok.iter().map(|r| r.rows_affected).sum::<u64>(), 2);
}

#[tokio::test]
async fn db_query_returns_typed_values() {
    let db = notes_db().await;
    db.execute(
        Statement::new("INSERT INTO note (id, body, n, r, b) VALUES (?, ?, ?, ?, ?)")
            .bind("x")
            .bind("ทดสอบ")
            .bind(1_767_225_600_000_i64)
            .bind(Value::Real(2.5))
            .bind(Value::Blob(vec![0, 255])),
    )
    .await
    .unwrap();
    let rows = db
        .query(Statement::new("SELECT body, n, r, b, NULL AS z FROM note"))
        .await
        .unwrap();
    assert_eq!(rows.columns, ["body", "n", "r", "b", "z"]);
    assert_eq!(rows.get(0, "body"), Some(&Value::Text("ทดสอบ".into())));
    assert_eq!(rows.get(0, "n"), Some(&Value::Integer(1_767_225_600_000)));
    assert_eq!(rows.get(0, "r"), Some(&Value::Real(2.5)));
    assert_eq!(rows.get(0, "b"), Some(&Value::Blob(vec![0, 255])));
    assert_eq!(rows.get(0, "z"), Some(&Value::Null));
}

#[tokio::test]
async fn db_file_persists_across_opens() {
    let dir = scratch("db");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.sqlite3");
    let path = path.to_str().unwrap();
    {
        let db = SqliteDb::open(path).unwrap();
        db.execute(Statement::new("CREATE TABLE t (v TEXT)"))
            .await
            .unwrap();
        db.execute(Statement::new("INSERT INTO t VALUES ('kept')"))
            .await
            .unwrap();
    }
    let db = SqliteDb::open(path).unwrap();
    let rows = db.query(Statement::new("SELECT v FROM t")).await.unwrap();
    assert_eq!(rows.get(0, "v").and_then(Value::as_text), Some("kept"));
    std::fs::remove_dir_all(dir).unwrap();
}

async fn round_trip(files: &dyn FileStore, key: &str) {
    assert_eq!(files.get(key).await.unwrap(), None);
    files
        .put(key, b"first".to_vec(), "text/plain")
        .await
        .unwrap();
    files
        .put(key, "แทนที่".as_bytes().to_vec(), "text/plain; charset=utf-8")
        .await
        .unwrap();
    assert_eq!(
        files.get(key).await.unwrap().as_deref(),
        Some("แทนที่".as_bytes())
    );
    files.delete(key).await.unwrap();
    assert_eq!(files.get(key).await.unwrap(), None);
    files.delete(key).await.unwrap(); // missing key: not an error
}

#[tokio::test]
async fn local_files_put_get_delete() {
    let dir = scratch("files");
    let files = LocalFiles::new(&dir);
    round_trip(&files, "roster/2026/import.csv").await;
    for bad in ["", "/etc/passwd", "../x", "a/../../x", "a//b", "a\\b"] {
        assert!(files.get(bad).await.is_err(), "{bad:?} accepted");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Runs only when `HARPSEAL_TEST_S3_BUCKET`, `HARPSEAL_TEST_S3_ENDPOINT`,
/// `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` are set
/// (`HARPSEAL_TEST_S3_REGION` defaults to `auto`).
#[tokio::test]
async fn s3_files_put_get_delete_when_configured() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let (Some(bucket), Some(endpoint), Some(id), Some(secret)) = (
        var("HARPSEAL_TEST_S3_BUCKET"),
        var("HARPSEAL_TEST_S3_ENDPOINT"),
        var("AWS_ACCESS_KEY_ID"),
        var("AWS_SECRET_ACCESS_KEY"),
    ) else {
        eprintln!("skipped: HARPSEAL_TEST_S3_* not set");
        return;
    };
    let region = var("HARPSEAL_TEST_S3_REGION").unwrap_or_else(|| "auto".into());
    let s = S3Settings::new(endpoint, bucket, region, id, secret);
    let http = Arc::new(harpseal_runtime_native::ReqwestHttp::new().unwrap());
    let files = S3Files::new(s, http, Arc::new(SystemClock));
    let key = scratch("s3");
    let key = format!(
        "harpseal-test/{}",
        key.file_name().unwrap().to_str().unwrap()
    );
    round_trip(&files, &key).await;
}

#[tokio::test]
async fn s3_requests_are_signed_path_style() {
    let http = harpseal_testing::RecordingHttp::default();
    let clock = harpseal_testing::ManualClock::new(1_767_225_600_000); // 2026-01-01
    let s = S3Settings::new(
        "https://acct.r2.cloudflarestorage.com/",
        "seal-bay",
        "auto",
        "AKIDEXAMPLE",
        "secret",
    );
    let files = S3Files::new(s, Arc::new(http.clone()), Arc::new(clock));
    http.respond_with(200, "");
    http.respond_with(404, "<Error><Code>NoSuchKey</Code></Error>");
    http.respond_with(204, "");
    files
        .put("a b/ใบ.txt", b"hi".to_vec(), "text/plain")
        .await
        .unwrap();
    assert_eq!(files.get("a b/ใบ.txt").await.unwrap(), None);
    files.delete("a b/ใบ.txt").await.unwrap();

    let reqs = http.requests();
    assert_eq!(reqs.len(), 3);
    let put = &reqs[0];
    assert_eq!(put.method, http::Method::PUT);
    assert_eq!(
        put.uri,
        "https://acct.r2.cloudflarestorage.com/seal-bay/a%20b/%E0%B9%83%E0%B8%9A.txt"
    );
    assert_eq!(put.headers["x-amz-date"], "20260101T000000Z");
    assert_eq!(put.headers["content-type"], "text/plain");
    let auth = put.headers["authorization"].to_str().unwrap();
    assert!(
        auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260101/auto/s3/aws4_request, \
             SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature="
        ),
        "{auth}"
    );
    assert_eq!(auth.rsplit('=').next().unwrap().len(), 64);
    assert_eq!(reqs[1].method, http::Method::GET);
    assert_eq!(reqs[2].method, http::Method::DELETE);
}

#[test]
fn system_clock_is_now_in_ms() {
    let now = SystemClock.now().ms();
    assert!(now > 1_767_225_600_000, "clock before 2026: {now}"); // 2026-01-01
    assert!(now < 4_102_444_800_000, "clock after 2100: {now}");
    assert!(SystemClock.now().ms() >= now);
}

#[test]
fn os_random_fills_and_differs() {
    let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
    OsRandom.fill(&mut a).unwrap();
    OsRandom.fill(&mut b).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, [0u8; 32]);
}

#[test]
fn settings_from_env_defaults_and_secrets() {
    let s = NativeSettings::from_lookup(|_| None).unwrap();
    assert_eq!(s.sqlite_path, "harpseal.sqlite3");
    assert_eq!(s.addr, "127.0.0.1:8080");
    assert!(matches!(&s.files, FilesSettings::Local(p) if p.as_os_str() == "files"));
    assert!(matches!(&s.email, EmailSettings::Smtp { url, .. } if url == "smtp://localhost:25"));

    let env = |k: &str| {
        let v = match k {
            "HARPSEAL_EMAIL" => "resend",
            "RESEND_API_KEY" => "re_secret",
            "HARPSEAL_S3_BUCKET" => "seal-bay",
            "HARPSEAL_S3_ENDPOINT" => "https://s3.example",
            "AWS_ACCESS_KEY_ID" => "id",
            "AWS_SECRET_ACCESS_KEY" => "very-secret",
            _ => return None,
        };
        Some(v.to_string())
    };
    let s = NativeSettings::from_lookup(env).unwrap();
    assert!(matches!(&s.email, EmailSettings::Resend { api_key, .. } if api_key == "re_secret"));
    assert!(matches!(&s.files, FilesSettings::S3(_)));
    let shown = format!("{s:?}");
    assert!(
        !shown.contains("re_secret") && !shown.contains("very-secret"),
        "{shown}"
    );

    let missing = NativeSettings::from_lookup(|k| (k == "HARPSEAL_EMAIL").then(|| "resend".into()));
    assert!(missing.unwrap_err().to_string().contains("RESEND_API_KEY"));
    let bad = NativeSettings::from_lookup(|k| (k == "HARPSEAL_EMAIL").then(|| "fax".into()));
    assert!(bad.is_err());
}
