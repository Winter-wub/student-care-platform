//! The roster preview shows what changes and for whom (#134): three
//! figures, the per-Student change list (old → new, labelled), the
//! leaving list with the Links each loses, a specific alarm the typed
//! confirmation names, the changes as an audited CSV, and the template's
//! example row refused.

mod roster_support;

use harpseal_testing::{Browser, Harness, TestApp};
use roster_support::*;

/// Five Students imported (68000000..68000004, year 1), plus "gone"
/// (68000900) with one live Link.
async fn roster_of_six(h: &Harness, app: &TestApp, b: &Browser<'_>) {
    let id = previewed(b, "first.csv", &csv(0..5), "upsert", false).await;
    confirm(b, &id).await;
    run_jobs(h, app).await;
    assert_eq!(import_text(h, &id, "state").await, "done");
    linked_student(h, "gone", "68000900", "active").await;
}

/// A full file: 68000000 moves to year 2, 1 and 2 stay, 68000010 is new;
/// 3, 4 and "gone" are not listed.
fn next_term() -> Vec<u8> {
    let mut lines =
        vec!["68000000,นักศึกษา 0,s68000000@student.sealbay.example,th,ปี1;MSC,MSC,2,3.0".to_string()];
    lines.extend([1, 2].map(|n| row(n).trim_end().to_string()));
    lines.push("68000010,นักศึกษา 10,s68000010@student.sealbay.example,th,ปี1;MSC,MSC,1,3.0".into());
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    csv_lines(&refs)
}

/// The text of the element after `marker` up to its next `<`.
fn after(page: &str, marker: &str) -> String {
    let at = page
        .find(marker)
        .unwrap_or_else(|| panic!("no {marker}: {page}"));
    let rest = &page[at + marker.len()..];
    let rest = &rest[rest.find('>').map_or(0, |i| i + 1)..];
    rest[..rest.find('<').unwrap_or(rest.len())].to_string()
}

#[tokio::test]
async fn the_preview_leads_with_what_changes_and_lists_who() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    roster_of_six(&h, &app, &b).await;
    let id = previewed(&b, "next.csv", &next_term(), "full", true).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;

    // Three figures under one heading; leaving in the refused style.
    assert!(
        page.contains("<h2 id=\"roster-what\">สิ่งที่จะเปลี่ยน</h2>"),
        "{page}"
    );
    assert_eq!(after(&page, "data-figure=\"new\"><dt>เพิ่มใหม่</dt><dd"), "1");
    assert_eq!(
        after(&page, "data-figure=\"updated\"><dt>แก้ไข</dt><dd"),
        "1"
    );
    assert!(
        page.contains("class=\"figure leaving\" id=\"roster-leaving\""),
        "{page}"
    );
    assert_eq!(after(&page, "data-count=\"will_become_left\""), "3");
    assert_eq!(
        after(&page, "data-count=\"links_cut\""),
        "ตัดการผูกบัญชีแชท 1 บัญชี"
    );
    // The other counts on the quieter line.
    assert_eq!(after(&page, "data-count=\"unchanged\""), "2");
    assert_eq!(after(&page, "data-count=\"active_before\""), "6");

    // The alarm in numbers, and the typed field tied to it and the figure.
    assert_eq!(
        after(&page, "<div class=\"warn\" id=\"roster-alarm\""),
        "3 จาก 6 คน (50%) จะพ้นสภาพ และถูกตัดการผูกบัญชีแชท 1 บัญชี \
         ตรวจให้แน่ใจว่าไฟล์นี้เป็นทะเบียนของภาคเรียนนี้ก่อนยืนยัน"
    );
    assert!(
        page.contains("aria-describedby=\"roster-alarm roster-leaving\""),
        "{page}"
    );

    // Who changes, collapsed, old → new with the column's name.
    assert!(page.contains("<details class=\"changes\" id=\"roster-changes\"><summary>ดูรายชื่อที่ข้อมูลจะเปลี่ยน (2 คน)</summary>"), "{page}");
    assert!(
        page.contains(
            "<span class=\"delta-field\">ชั้นปี</span> <span class=\"delta-before\">1</span>\
             <span class=\"delta-arrow\" aria-hidden=\"true\"> → </span>"
        ),
        "{page}"
    );
    assert!(
        page.contains(
            "<tr data-kind=\"new\"><td class=\"code id\" data-label=\"รหัสนักศึกษา\">68000010</td>"
        ),
        "{page}"
    );

    // Who leaves, by code, with the Links each loses.
    let list = &page[page
        .find("id=\"roster-leaving-list\"")
        .expect("the leaving list")..];
    let gone = &list[list.find(">68000900<").expect("gone is listed")..];
    assert!(
        gone.contains("Old gone</td><td class=\"num\" data-label=\"บัญชีแชทที่จะถูกตัด\">1</td>"),
        "{gone}"
    );
    assert!(list.find(">68000003<") < list.find(">68000900<"), "by code");

    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
    // The row's counts name no Student: the lists are in the FileStore.
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["links_cut"], 1);
    let raw = import_text(&h, &id, "counts").await;
    assert!(
        !raw.contains("Old gone") && !raw.contains("68000900"),
        "{raw}"
    );
    assert!(
        h.files()
            .keys()
            .contains(&format!("roster-diff/{id}.head.json"))
    );
}

#[tokio::test]
async fn the_import_audit_row_keeps_the_counts_and_no_student() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    roster_of_six(&h, &app, &b).await;
    let id = previewed(&b, "next.csv", &next_term(), "full", true).await;
    let r = confirm(&b, &id).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let rows = h
        .db()
        .query(
            harpseal::Statement::new(
                "SELECT after FROM audit_log WHERE action = 'roster.import' AND target_id = ?",
            )
            .bind(id.as_str()),
        )
        .await
        .unwrap();
    let after = rows
        .get(0, "after")
        .and_then(|v| v.as_text())
        .unwrap_or_default()
        .to_string();
    let json: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(json["counts"]["will_become_left"], 3, "{after}");
    assert_eq!(json["counts"]["updated"], 1, "{after}");
    assert!(json["counts"].get("left_names").is_none(), "{after}");
    for student in ["68000900", "Old gone", "นักศึกษา 0", "68000010"] {
        assert!(!after.contains(student), "{student}: {after}");
    }
}

#[tokio::test]
async fn an_alarm_with_no_links_to_cut_does_not_count_them() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..5), "upsert", false).await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let id = previewed(&b, "b.csv", &csv(0..2), "full", true).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert_eq!(
        after(&page, "<div class=\"warn\" id=\"roster-alarm\""),
        "3 จาก 5 คน (60%) จะพ้นสภาพ ตรวจให้แน่ใจว่าไฟล์นี้เป็นทะเบียนของภาคเรียนนี้ก่อนยืนยัน"
    );
    assert!(!page.contains("data-count=\"links_cut\""), "{page}");
    // The file's own rows do not change; the sentence says only that.
    assert!(page.contains("ข้อมูลของนักศึกษาที่อยู่ในไฟล์ไม่เปลี่ยน"), "{page}");
}

#[tokio::test]
async fn the_changes_download_as_an_audited_csv() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    roster_of_six(&h, &app, &b).await;
    let id = previewed(&b, "next.csv", &next_term(), "full", true).await;
    let viewer = signed_in(&h, &app, "Viewer").await;
    let r = viewer.get(&format!("/admin/roster/{id}/changes.csv")).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("text/csv; charset=utf-8"));
    assert!(
        r.body.starts_with(
            "\u{feff}รหัสนักศึกษา,ชื่อ-สกุล,การเปลี่ยนแปลง,ข้อมูล,ค่าเดิม,ค่าใหม่,บัญชีแชทที่จะถูกตัด\r\n"
        ),
        "{}",
        r.body
    );
    assert!(
        r.body.contains("68000000,นักศึกษา 0,แก้ไข,ชั้นปี,1,2,\r\n"),
        "{}",
        r.body
    );
    assert!(
        r.body.contains("68000010,นักศึกษา 10,เพิ่มใหม่,,,,\r\n"),
        "{}",
        r.body
    );
    assert!(
        r.body.contains("68000900,Old gone,จะพ้นสภาพ,,,,1\r\n"),
        "{}",
        r.body
    );
    assert_eq!(r.body.lines().count(), 6, "{}", r.body);
    let sql = "SELECT count(*) FROM audit_log WHERE action = 'roster.changes.download'";
    assert_eq!(int(&h, sql, &[]).await, 1);
    // A list that went missing is gone, never answered in part.
    h.services()
        .files
        .delete(&format!("roster-diff/{id}.jsonl"))
        .await
        .unwrap();
    let again = viewer.get(&format!("/admin/roster/{id}/changes.csv")).await;
    assert_eq!(again.status, 410, "{}", again.body);
    assert_eq!(
        int(&h, sql, &[]).await,
        1,
        "a refused download is not audited"
    );
}

#[tokio::test]
async fn an_upsert_preview_says_no_one_leaves() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    roster_of_six(&h, &app, &b).await;
    let id = previewed(&b, "next.csv", &next_term(), "upsert", false).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("แบบเพิ่ม/แก้ไขไม่ทำให้ใครพ้นสภาพ"), "{page}");
    assert!(!page.contains("id=\"roster-leaving-list\""), "{page}");
    assert!(!page.contains("id=\"roster-alarm\""), "{page}");
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_file_that_changes_nothing_says_so() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "a.csv", &csv(0..2), "upsert", false).await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    let id = previewed(&b, "a.csv", &csv(0..2), "upsert", false).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("ข้อมูลของนักศึกษาที่อยู่ในไฟล์ไม่เปลี่ยน"), "{page}");
    assert!(!page.contains("id=\"roster-changes\""), "{page}");
}

#[tokio::test]
async fn the_templates_example_row_is_refused_by_row() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let template = b.get("/admin/roster/template.csv").await.body;
    let mut file = template.trim_start_matches('\u{feff}').to_string();
    file.push_str(&row(1));
    let r = upload(&b, "filled.csv", file.as_bytes(), "upsert").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(page.contains("แถวนี้เป็นแถวตัวอย่างจากแม่แบบ"), "{page}");
    assert!(page.contains("data-label=\"แถว\">2</td>"), "{page}");
    assert_eq!(int(&h, "SELECT count(*) FROM student", &[]).await, 0);
}

#[tokio::test]
async fn the_example_row_is_named_even_when_the_code_pattern_refuses_it_and_keeps_the_warnings() {
    // A pattern of 9 digits refuses the example's code (68010001), yet the
    // row is still named as the example: it is found before validation.
    let h = Harness::new();
    let toml = CONFIG.replace("id_pattern = '########'", "id_pattern = '#########'");
    let config = harpseal::DeploymentConfig::from_toml(&toml).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv_lines(&[
        "68010001,นกน้อย ทะเลใส,noknoi@student.example,th,ชมรมดนตรี;ทุนเรียนดี,MSC,1,2.50",
        "680000002,ปลา,,th,,MSC,1,3.0",
    ]);
    let id = import_id(&upload(&b, "t.csv", &file, "upsert").await);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("2,,แถวนี้เป็นแถวตัวอย่างจากแม่แบบ"), "{report}");
    assert!(
        report.contains("3,อีเมล,ไม่มีอีเมล"),
        "the warning stays: {report}"
    );
}

#[tokio::test]
async fn a_preview_whose_lists_are_gone_says_so_beside_the_figures() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    roster_of_six(&h, &app, &b).await;
    let id = previewed(&b, "next.csv", &next_term(), "full", true).await;
    // As after a restored backup, which carries the file but not the lists.
    for key in [
        format!("roster-diff/{id}.jsonl"),
        format!("roster-diff/{id}.head.json"),
    ] {
        h.services().files.delete(&key).await.unwrap();
    }
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    // The figures stay; the lists give way to one line, never to nothing.
    assert_eq!(after(&page, "data-count=\"will_become_left\""), "3");
    assert!(
        page.contains("ไม่มีรายชื่อของการนำเข้านี้ให้ดูแล้ว อัปโหลดไฟล์อีกครั้ง"),
        "{page}"
    );
    assert!(!page.contains("id=\"roster-changes\""), "{page}");
    assert!(!page.contains("id=\"roster-leaving-list\""), "{page}");
    assert_eq!(page.matches("id=\"roster-lists-missing\"").count(), 1);
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
    // The download says it is not available, whatever the reason.
    let r = b.get(&format!("/admin/roster/{id}/changes.csv")).await;
    assert_eq!(r.status, 410);
    assert!(
        r.body
            .contains("รายการเปลี่ยนแปลงของการนำเข้านี้ไม่มีให้ดาวน์โหลดแล้ว"),
        "{}",
        r.body
    );
}

#[tokio::test]
async fn a_cancelled_or_replaced_preview_leaves_no_lists_behind() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let keys = |id: &str| {
        [
            format!("roster-diff/{id}.jsonl"),
            format!("roster-diff/{id}.head.json"),
        ]
    };
    let stored = |id: &str| {
        keys(id)
            .iter()
            .filter(|k| h.files().keys().contains(k))
            .count()
    };
    // Cancelled by Staff.
    let id = previewed(&b, "a.csv", &csv(0..3), "full", false).await;
    assert_eq!(stored(&id), 2);
    b.get(&format!("/admin/roster/{id}")).await;
    b.post(&format!("/admin/roster/{id}/cancel"), "").await;
    assert_eq!(import_text(&h, &id, "state").await, "cancelled");
    assert_eq!(stored(&id), 0);
    // Replaced after an hour by the next upload.
    let old = previewed(&b, "a.csv", &csv(0..3), "full", false).await;
    assert_eq!(stored(&old), 2);
    h.clock().advance_minutes(61);
    let new = previewed(&b, "b.csv", &csv(0..3), "full", false).await;
    assert_eq!(import_text(&h, &old, "state").await, "cancelled");
    assert_eq!(stored(&old), 0);
    assert_eq!(stored(&new), 2, "the new preview keeps its own");
}
