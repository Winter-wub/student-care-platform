//! Roster upload edge cases over HTTP (#24 review): where the `_csrf` part
//! sits, body sizes around axum's 2 MB default and the 4 MiB limit, files
//! that cannot be read, an empty full file, the truncated report and the
//! audited replacement of a stale preview.

mod roster_support;

use roster_support::*;

#[tokio::test]
async fn the_csrf_part_may_come_after_the_file_but_must_be_there() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let none = upload_ordered(&b, "a.csv", &csv(0..3), "full", Some(""), false).await;
    assert_eq!(none.status, 403, "no _csrf part at all");
    let last = upload_ordered(&b, "a.csv", &csv(0..3), "full", None, true).await;
    assert_eq!(last.status, 303, "{}", last.body);
    assert_eq!(flash(&b), "ok:kernel.roster.previewed");
}

/// A valid roster of about `bytes` bytes (long names, 4 500 rows for 3 MiB).
fn big_roster(bytes: usize) -> Vec<u8> {
    let mut s = format!("{HEADERS}\r\n");
    let mut n = 0;
    while s.len() < bytes {
        let code = 68_000_000 + n;
        let name = format!("{} {n}", "ก".repeat(190));
        s.push_str(&format!("{code},{name},,th,,MSC,1,\r\n"));
        n += 1;
    }
    s.into_bytes()
}

#[tokio::test]
async fn a_3_mib_file_is_accepted_and_one_over_4_mib_refused() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let big = big_roster(3 * 1024 * 1024);
    assert!(
        big.len() > 2 * 1024 * 1024,
        "past axum's default body limit"
    );
    let id = previewed(&b, "big.csv", &big, "upsert", false).await;
    assert!(
        int(
            &h,
            "SELECT total_rows FROM roster_import WHERE id = ?",
            &[&id]
        )
        .await
            > 4000
    );
    b.get(&format!("/admin/roster/{id}")).await;
    b.post(&format!("/admin/roster/{id}/cancel"), "").await;

    let too_big = vec![b'a'; 4 * 1024 * 1024 + 10_000];
    let r = upload(&b, "huge.csv", &too_big, "full").await;
    assert_eq!(r.status, 303);
    assert_eq!(flash(&b), "err:kernel.roster.bad_upload");
    assert_eq!(int(&h, "SELECT count(*) FROM roster_import", &[]).await, 1);
}

async fn refused_reason(name: &str, bytes: &[u8], mode: &str) -> String {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(&b, name, bytes, mode).await;
    assert_eq!(flash(&b), "err:kernel.roster.flash_refused", "{}", r.body);
    let id = import_id(&r);
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("ไม่นำเข้าเลย"));
    import_text(&h, &id, "error").await
}

#[tokio::test]
async fn csv_that_is_not_utf8_or_has_broken_quotes_is_refused() {
    let mut latin1 = csv(0..2);
    latin1.push(0xE9);
    assert!(
        refused_reason("a.csv", &latin1, "full")
            .await
            .contains("not_utf8")
    );
    let broken = csv_lines(&["68000001,\"นก,,,,MSC,1,"]);
    assert!(
        refused_reason("a.csv", &broken, "full")
            .await
            .contains("bad_csv")
    );
}

#[tokio::test]
async fn a_full_file_without_rows_is_refused_and_an_upsert_one_is_not() {
    let empty = csv(0..0);
    assert!(
        refused_reason("a.csv", &empty, "full")
            .await
            .contains("empty_full")
    );
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &empty, "upsert", false).await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
}

#[tokio::test]
async fn a_long_report_says_how_many_problems_it_leaves_out() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let lines: Vec<String> = (0..1100)
        .map(|n| format!("x{n},นก,x{n}@student.sealbay.example,,,MSC,1,3.0"))
        .collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let r = upload(&b, "bad.csv", &csv_lines(&refs), "full").await;
    let id = import_id(&r);
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert_eq!(report.lines().count(), 1 + 1000 + 1);
    assert_eq!(report.lines().last(), Some("แสดง 1000 จาก 1100 ปัญหา"));
}

#[tokio::test]
async fn replacing_a_stale_preview_is_audited() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let old = previewed(&b, "a.csv", &csv(0..3), "full", false).await;
    h.clock().advance_minutes(61);
    previewed(&b, "b.csv", &csv(0..3), "full", false).await;
    let audit = "SELECT count(*) FROM audit_log WHERE action = 'roster.import.cancel' \
                 AND target_id = ? AND actor_kind = 'system'";
    assert_eq!(int(&h, audit, &[&old]).await, 1);
}
