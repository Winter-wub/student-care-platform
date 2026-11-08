//! A table read across nightly Job steps, with writes in between (#88
//! review): a Link revoked and a new live Link made for the same Student
//! after the first was read would leave two live Links in the backup,
//! which `link_live_student_channel` refuses on restore. The dump reads
//! such rows again before the table ends, so the backup marked done
//! restores, with the Link as it was when read again.

mod backup_support;

use backup_support::nightly::{NIGHT, stamp_of, ticks};
use backup_support::{deployment, populated_as, query};
use harpseal::backup::{self, FileStoreBackup, LoadOptions};
use harpseal::{FileStore, Statement};
use harpseal_testing::{Harness, TestApp};
use serde_json::Value as Json;
use std::sync::Arc;

/// Rows of `link`: more than one step's 50,000, so it spans two steps.
const LINKS: i64 = 60_000;

async fn exec(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// 60,000 Students, each with a live Link, and Student `s-special` whose
/// live Link `!first` sorts before every other Link (so it is read first).
async fn many_links(h: &Harness) {
    let n = format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {LINKS})"
    );
    exec(h, &format!(
        "{n} INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
         SELECT printf('s-%06d', i), printf('69%06d', i), printf('นักศึกษา %06d', i), 'th', 1, 1 FROM n",
    ))
    .await;
    exec(
        h,
        &format!(
            "{n} INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
         SELECT printf('ca-%06d', i), 'fake', printf('u-many-%06d', i), 1, 1 FROM n",
        ),
    )
    .await;
    exec(
        h,
        &format!(
            "{n} INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
         linked_at, updated_at) \
         SELECT printf('l-%06d', i), printf('s-%06d', i), printf('ca-%06d', i), 'fake', \
         'active', 'manual', 1, 1 FROM n",
        ),
    )
    .await;
    exec(
        h,
        "INSERT INTO student (id, student_code, display_name, locale, created_at, updated_at) \
             VALUES ('s-special', '68999999', 'ปลาวาฬ ทะเลลึก', 'th', 1, 1)",
    )
    .await;
    for ca in ["ca-special-1", "ca-special-2"] {
        exec(
            h,
            &format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, updated_at) \
             VALUES ('{ca}', 'fake', 'u-{ca}', 1, 1)"
            ),
        )
        .await;
    }
    exec(
        h,
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('!first', 's-special', 'ca-special-1', 'fake', \
             'active', 'manual', 1, 1)",
    )
    .await;
}

/// The saved progress of the run at [`NIGHT`], if any.
async fn progress(h: &Harness) -> Option<Json> {
    let key = format!("backups/{}.progress.json", stamp_of(NIGHT));
    let raw = h.files().get(&key).await.unwrap()?;
    Some(serde_json::from_slice(&raw).unwrap())
}

/// Ticks until the run has stopped inside `link`; `true` once it has.
async fn until_inside_link(h: &Harness, app: &TestApp) -> bool {
    for _ in 0..20 {
        ticks(h, app, 1).await;
        let Some(p) = progress(h).await else { continue };
        let done = p["tables"].as_array().unwrap().len();
        let current = &p["order"][done];
        let parts = p["cursor"]["parts"].as_array().map_or(0, Vec::len);
        if current == "link" && parts > 0 {
            return true;
        }
    }
    false
}

#[tokio::test]
async fn a_relink_between_two_steps_of_one_table_is_read_again_and_restores() {
    let h = Harness::new();
    let d = deployment(&h);
    let (h, app) = populated_as(h, d).await;
    many_links(&h).await;

    h.clock().set_ms(NIGHT);
    assert!(
        until_inside_link(&h, &app).await,
        "the run stopped inside link"
    );
    // Between two steps: `!first` (already read) is revoked, and the same
    // Student links again; the new Link sorts last, so it is read next.
    exec(
        &h,
        "UPDATE link SET status = 'revoked', revoked_at = 2, updated_at = 2 \
              WHERE id = '!first'",
    )
    .await;
    exec(
        &h,
        "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
              linked_at, updated_at) VALUES ('~again', 's-special', 'ca-special-2', 'fake', \
              'active', 'manual', 2, 2)",
    )
    .await;
    ticks(&h, &app, 15).await;

    let row = query(h.db().as_ref(), "SELECT state, error FROM backup_run").await;
    assert_eq!(row.rows[0][0].as_text(), Some("done"), "{:?}", row.rows[0]);
    assert!(
        !h.files().keys().iter().any(|k| k.contains(".check/")),
        "the saved key hashes are deleted"
    );
    let files: Arc<dyn FileStore> = Arc::new(h.files().clone());
    let b = FileStoreBackup::new(files, format!("backups/{}/", stamp_of(NIGHT)));
    let fresh = Harness::new();
    let d = deployment(&fresh);
    backup::load(&d, &b, &LoadOptions::new()).await.unwrap();
    let links = query(
        fresh.db().as_ref(),
        "SELECT id, status FROM link WHERE student_id = 's-special' ORDER BY id",
    )
    .await;
    let got: Vec<(String, String)> = links
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
        got,
        [
            ("!first".to_string(), "revoked".to_string()),
            ("~again".to_string(), "active".to_string())
        ]
    );
    let n = query(fresh.db().as_ref(), "SELECT count(*) FROM link").await;
    assert_eq!(n.rows[0][0].as_i64(), Some(LINKS + 3));
}
