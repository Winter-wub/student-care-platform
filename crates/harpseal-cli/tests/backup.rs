//! `harpseal backup` / `restore` on a directory (#53).

use std::path::PathBuf;

use harpseal_cli::{CliError, Command, parse, run};
use harpseal_testing::Harness;

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// A new, not yet existing path under the system temp directory.
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("harpseal-cli-{tag}-{}-{nanos}", std::process::id()))
}

async fn cli(d: &harpseal::Deployment, line: &str) -> Result<String, CliError> {
    let mut out = Vec::new();
    run(d, &args(line), &mut &b""[..], &mut out).await?;
    Ok(String::from_utf8(out).unwrap())
}

#[tokio::test]
async fn backup_and_restore_round_trip_through_a_directory() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let line = "staff create --admin --email root@sealbay.example --password kelp-forest-at-dawn";
    cli(&d, line).await.unwrap();

    let dir = temp_dir("roundtrip");
    let text = cli(&d, &format!("backup {}", dir.display())).await.unwrap();
    assert!(text.contains("table staff: 1 rows"), "{text}");
    assert!(text.contains("backup written to"), "{text}");
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join("tables/staff.ndjson").is_file());
    assert!(!dir.join("tables/migration.ndjson").exists());
    let mut partial = dir.as_os_str().to_owned();
    partial.push(".partial");
    assert!(!PathBuf::from(partial).exists(), "renamed into place");

    // Restoring onto the same, non-empty database is refused without --force.
    let refused = cli(&d, &format!("restore {}", dir.display())).await;
    assert!(
        matches!(&refused, Err(CliError::Core(harpseal::Error::Conflict(_)))),
        "{refused:?}"
    );
    let forced = cli(&d, &format!("restore {} --force", dir.display()))
        .await
        .unwrap();
    assert!(forced.contains("--force: deleted the rows"), "{forced}");

    // Into a fresh database: the Administrator signs in with the same password.
    let fresh = Harness::new();
    let d2 = fresh.builder().build().unwrap();
    let text = cli(&d2, &format!("restore {}", dir.display()))
        .await
        .unwrap();
    assert!(text.contains("table staff: 1 rows"), "{text}");
    assert!(text.contains("audit backup.restore"), "{text}");
    // Same seeded Random from the same instant would repeat restored ids.
    fresh.clock().advance_minutes(24 * 60);
    let app = fresh.boot(d2).await;
    let r = app
        .browser()
        .login("root@sealbay.example", "kelp-forest-at-dawn")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn backup_refuses_a_non_empty_directory() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    d.migrate().await.unwrap();
    let dir = temp_dir("nonempty");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "keep me").unwrap();
    let r = cli(&d, &format!("backup {}", dir.display())).await;
    assert!(
        matches!(&r, Err(CliError::Refused(m)) if m.contains("not empty")),
        "{r:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "keep me"
    );

    // An empty existing directory is fine.
    let empty = temp_dir("empty");
    std::fs::create_dir_all(&empty).unwrap();
    cli(&d, &format!("backup {}", empty.display()))
        .await
        .unwrap();
    assert!(empty.join("manifest.json").is_file());
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&empty).unwrap();
}

#[test]
fn backup_and_restore_command_lines() {
    assert_eq!(
        parse(&args("backup /srv/b")).unwrap(),
        Command::Backup("/srv/b".into())
    );
    for line in ["restore /srv/b --force", "restore --force /srv/b"] {
        assert_eq!(
            parse(&args(line)).unwrap(),
            Command::Restore {
                dir: "/srv/b".into(),
                force: true
            },
            "{line}"
        );
    }
    assert_eq!(
        parse(&args("restore /srv/b")).unwrap(),
        Command::Restore {
            dir: "/srv/b".into(),
            force: false
        }
    );
    for line in [
        "backup",
        "restore",
        "backup a b",
        "restore a --wipe",
        "backup --force",
    ] {
        assert!(
            matches!(parse(&args(line)), Err(CliError::Usage(_))),
            "{line}"
        );
    }
}
