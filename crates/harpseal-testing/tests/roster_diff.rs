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
        "ตัดการเชื่อมบัญชีแชท 1 บัญชี"
    );
    // The other counts on the quieter line.
    assert_eq!(after(&page, "data-count=\"unchanged\""), "2");
    assert_eq!(after(&page, "data-count=\"active_before\""), "6");

    // The alarm in numbers, and the typed field tied to it and the figure.
    assert_eq!(
        after(&page, "<div class=\"warn\" id=\"roster-alarm\""),
        "3 จาก 6 คน (50%) จะพ้นสภาพ และถูกตัดการเชื่อมบัญชีแชท 1 บัญชี \
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
            "<tr data-kind=\"new\"><td class=\"code\" data-label=\"รหัสนักศึกษา\">68000010</td>"
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
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["links_cut"], 1);
    assert_eq!(c["preview"]["leaving"].as_array().unwrap().len(), 3);
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
    // The stored list goes; the capped lists still answer.
    run(
        &h,
        &format!("UPDATE roster_import SET file_key = '' WHERE id = '{id}'"),
    )
    .await;
    h.services()
        .files
        .delete(&format!("roster-diff/{id}.jsonl"))
        .await
        .unwrap();
    let again = viewer.get(&format!("/admin/roster/{id}/changes.csv")).await;
    assert_eq!(again.body.lines().count(), 6, "{}", again.body);
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
    assert!(page.contains("ไฟล์นี้ไม่ทำให้ข้อมูลของใครเปลี่ยน"), "{page}");
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
