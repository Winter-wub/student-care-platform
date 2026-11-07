//! Native runtime (#78): the nightly backup Job writes into the FileStore
//! directory, and `harpseal restore` reads that directory as it is, and
//! the `.tar` the Admin page serves once unpacked.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harpseal::{Deployment, DeploymentConfig, Statement, Timestamp};
use harpseal_cli::run;
use harpseal_runtime_native::LocalFiles;
use harpseal_testing::{Harness, START_MS};

const TOML: &str = "name = \"Seal Bay University\"\ndefault_locale = \"th\"\n\
                    utc_offset = \"+07:00\"\n";
const ADMIN: &str = "root@sealbay.example";
const PASSWORD: &str = "kelp-forest-at-dawn";
/// 02:00 in Bangkok: the default `[backup] schedule`.
const NIGHT: i64 = START_MS + 19 * 3_600_000;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "harpseal-nightly-{tag}-{}-{nanos}",
        std::process::id()
    ))
}

/// The harness's Deployment with its FileStore in the directory `root`.
fn deployment(h: &Harness, root: &Path) -> Deployment {
    let mut services = h.services();
    services.files = Arc::new(LocalFiles::new(root));
    let config = DeploymentConfig::from_toml(TOML).unwrap();
    h.builder()
        .config(config)
        .runtime(services)
        .build()
        .unwrap()
}

async fn cli(d: &Deployment, line: &str) -> String {
    let args: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    let mut out = Vec::new();
    run(d, &args, &mut &b""[..], &mut out)
        .await
        .unwrap_or_else(|e| panic!("{line}: {e}"));
    String::from_utf8(out).unwrap()
}

async fn staff(h: &Harness) -> i64 {
    let rows = h
        .db()
        .query(Statement::new("SELECT count(*) FROM staff"))
        .await
        .unwrap();
    rows.rows[0][0].as_i64().unwrap()
}

#[tokio::test]
async fn a_nightly_backup_restores_with_harpseal_restore() {
    let h = Harness::new();
    let root = temp_dir("files");
    let d = deployment(&h, &root);
    let line = format!("staff create --admin --email {ADMIN} --password {PASSWORD}");
    cli(&d, &line).await;
    cli(
        &d,
        "staff create --email officer@sealbay.example --password kelp-forest-at-noon",
    )
    .await;
    let app = h.boot(d).await;

    h.clock().set_ms(NIGHT);
    for _ in 0..10 {
        app.deployment()
            .tick(Timestamp(h.clock().now_ms()))
            .await
            .unwrap();
        h.clock().advance_minutes(1);
    }
    let dir = root.join("backups/20261105T190000000Z");
    assert!(dir.join("manifest.json").is_file(), "{}", dir.display());
    assert!(dir.join("tables/staff.ndjson").is_file());
    assert!(
        !root
            .join("backups/20261105T190000000Z.progress.json")
            .exists()
    );

    // Straight from the FileStore directory.
    let fresh = Harness::new();
    fresh.clock().advance_minutes(3 * 24 * 60);
    let d2 = deployment(&fresh, &temp_dir("restored"));
    let out = cli(&d2, &format!("restore {}", dir.display())).await;
    assert!(out.contains("table staff: 2 rows"), "{out}");
    assert_eq!(staff(&fresh).await, 2);

    // Through the Admin page's .tar, unpacked with the system tar.
    let b = app.browser();
    assert_eq!(b.login(ADMIN, PASSWORD).await.status, 303);
    let id = app.query("SELECT id FROM backup_run").await.rows[0][0]
        .as_text()
        .unwrap()
        .to_string();
    let r = b.get(&format!("/admin/backups/{id}/download")).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let unpack = temp_dir("tar");
    std::fs::create_dir_all(&unpack).unwrap();
    let tar = unpack.join("backup.tar");
    std::fs::write(&tar, &r.bytes).unwrap();
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&tar)
        .current_dir(&unpack)
        .status()
        .unwrap();
    assert!(status.success());
    let third = Harness::new();
    third.clock().advance_minutes(3 * 24 * 60);
    let d3 = deployment(&third, &temp_dir("restored-tar"));
    let unpacked = unpack.join("20261105T190000000Z");
    let out = cli(&d3, &format!("restore {}", unpacked.display())).await;
    assert!(out.contains("restored"), "{out}");
    assert_eq!(staff(&third).await, 2);
    for p in [root, unpack] {
        let _ = std::fs::remove_dir_all(p);
    }
}
