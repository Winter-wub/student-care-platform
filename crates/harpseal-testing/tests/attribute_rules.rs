//! Roster attribute rules and labels (#81) over HTTP: `min` / `max` on an
//! integer, display labels on an enum. The roster import (out of range is
//! blocking, a label is read as its value), the Students pages and the edit
//! form. The Audience form is in `attribute_rules_audience.rs`.

mod roster_support;

use harpseal::{DeploymentConfig, Locale, NewStaff, Statement};
use harpseal_testing::{Browser, Harness, TestApp, urlencode};
use roster_support::{
    confirm, flash, import_id, import_text, int, run, run_jobs, signed_in, upload,
};

const HEADERS: &str = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,ภาษา,กลุ่ม,สถานะ,ชั้นปี";

const CONFIG: &str = r#"
name = "Seal Bay University (attribute rules)"
default_locale = "th"
utc_offset = "+07:00"

[student]
id_pattern = '########'

[roles]
Officer = ["kernel.student.view", "kernel.student.edit", "kernel.link.manage",
           "kernel.roster.view", "kernel.roster.import"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"
locale = { column = "ภาษา", default = "th" }
tags = { column = "กลุ่ม", separator = ";" }

[roster.attributes.status]
column = "สถานะ"
type = "enum"
values = ["current", "graduated", "leave"]
value_labels = { current = { th = "กำลังศึกษา", en = "Studying" }, graduated = { th = "จบการศึกษา" }, leave = { th = "ลาพัก", en = "On leave" } }
label = { th = "สถานะ", en = "Status" }
shown_in_list = true
order = 1

[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
min = 1
max = 6
label = { th = "ชั้นปี", en = "Year" }
shown_in_list = true
order = 2
"#;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    (h, app)
}

fn csv(lines: &[&str]) -> Vec<u8> {
    let mut s = format!("{HEADERS}\r\n");
    for l in lines {
        s.push_str(l);
        s.push_str("\r\n");
    }
    s.into_bytes()
}

async fn student(h: &Harness, id: &str, attrs: &str) {
    run(
        h,
        &format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             attributes, created_at, updated_at) VALUES ('{id}', '6801000{id}', 'Nok {id}', \
             'nok{id}@student.sealbay.example', 'th', 'active', '{attrs}', 1, 1)"
        ),
    )
    .await;
}

/// The stored `status` of the Student with `code`.
async fn status_of(h: &Harness, code: &str) -> Option<String> {
    let sql =
        "SELECT json_extract(attributes, '$.status') AS s FROM student WHERE student_code = ?";
    let rows = h.db().query(Statement::new(sql).bind(code)).await.unwrap();
    rows.get(0, "s")
        .and_then(|v| v.as_text())
        .map(str::to_string)
}

/// Uploads a file that must be refused; returns the report CSV.
async fn refused(b: &Browser<'_>, h: &Harness, file: &[u8]) -> String {
    let r = upload(b, "a.csv", file, "full").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(b), "err:kernel.roster.flash_refused");
    let id = import_id(&r);
    assert_eq!(import_text(h, &id, "state").await, "failed");
    assert_eq!(int(h, "SELECT count(*) FROM student", &[]).await, 0);
    let page = b.get(&format!("/admin/roster/{id}")).await;
    assert!(page.body.contains("ค่าอยู่นอกช่วงที่กำหนด") || page.body.contains("ไม่อยู่ในค่า"));
    b.get(&format!("/admin/roster/{id}/report.csv")).await.body
}

#[tokio::test]
async fn an_out_of_range_year_blocks_the_import_with_its_row_and_the_range() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv(&[
        "68000001,นก,,,,current,6",
        "68000002,ปลา,,,,current,7",
        "68000003,หอย,,,,current,0",
        "68000004,ปู,,,,current,1",
    ]);
    let report = refused(&b, &h, &file).await;
    assert!(report.contains("3,ชั้นปี,ค่าอยู่นอกช่วงที่กำหนด: 1–6"), "{report}");
    assert!(report.contains("4,ชั้นปี,ค่าอยู่นอกช่วงที่กำหนด: 1–6"), "{report}");
    assert!(!report.contains("2,ชั้นปี"), "6 is inside: {report}");
    assert!(!report.contains("5,ชั้นปี"), "1 is inside: {report}");
}

#[tokio::test]
async fn a_cell_may_name_the_value_by_its_label_and_the_value_is_stored() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = csv(&[
        "68000001,นก,,,,กำลังศึกษา,1",
        "68000002,ปลา,,,, จบการศึกษา ,2",
        "68000003,หอย,,,,graduated,3",
        "68000004,ปู,,,,ลาพัก,4",
    ]);
    let r = upload(&b, "a.csv", &file, "full").await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.roster.previewed");
    let id = import_id(&r);
    assert_eq!(confirm(&b, &id).await.status, 303);
    run_jobs(&h, &app).await;
    assert_eq!(status_of(&h, "68000001").await.as_deref(), Some("current"));
    assert_eq!(
        status_of(&h, "68000002").await.as_deref(),
        Some("graduated")
    );
    assert_eq!(
        status_of(&h, "68000003").await.as_deref(),
        Some("graduated")
    );
    assert_eq!(status_of(&h, "68000004").await.as_deref(), Some("leave"));
}

#[tokio::test]
async fn only_the_default_locale_label_is_read_and_the_report_lists_the_choices() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    // `Studying` is the English label; the import reads the Thai one.
    let report = refused(&b, &h, &csv(&["68000001,นก,,,,Studying,1"])).await;
    assert!(
        report.contains(
            "2,สถานะ,\"ไม่อยู่ในค่าที่อนุญาต: กำลังศึกษา (current), จบการศึกษา (graduated), ลาพัก (leave)\",ต้องแก้"
        ),
        "{report}"
    );
}

#[tokio::test]
async fn the_import_page_lists_each_value_with_its_label_and_the_range() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/roster").await.body;
    assert!(page.contains("กำลังศึกษา (current)"), "{page}");
    assert!(page.contains("(1–6)"), "{page}");
}

#[tokio::test]
async fn the_students_pages_show_labels_while_the_stored_value_stays() {
    let (h, app) = boot().await;
    student(&h, "1", r#"{"status":"current","year":2}"#).await;
    student(&h, "2", r#"{"status":"graduated","year":6}"#).await;
    student(&h, "3", r#"{"status":"retired","year":3}"#).await;
    let b = signed_in(&h, &app, "Officer").await;

    let list = b.get("/admin/students").await.body;
    assert!(list.contains("กำลังศึกษา"), "{list}");
    assert!(list.contains("จบการศึกษา"), "{list}");
    assert!(
        !list.contains(">current<") && !list.contains(">graduated<"),
        "{list}"
    );
    assert!(
        list.contains(">retired<"),
        "a value without a label shows as it is"
    );

    let detail = b.get("/admin/students/1").await.body;
    assert!(detail.contains("<dd>กำลังศึกษา</dd>"), "{detail}");

    let edit = b.get("/admin/students/1/edit").await.body;
    assert!(
        edit.contains(r#"<option value="current" selected>กำลังศึกษา</option>"#),
        "{edit}"
    );
    assert!(
        edit.contains(r#"<option value="leave">ลาพัก</option>"#),
        "{edit}"
    );
    assert!(edit.contains(r#"<span class="muted">1–6</span>"#), "{edit}");

    let row = app
        .query("SELECT attributes FROM student WHERE id = '1'")
        .await;
    let text = row
        .get(0, "attributes")
        .unwrap()
        .as_text()
        .unwrap()
        .to_string();
    assert!(text.contains(r#""status":"current""#), "{text}");
}

#[tokio::test]
async fn a_staff_member_reads_the_label_in_their_locale_else_the_default_locale() {
    let (h, app) = boot().await;
    student(&h, "1", r#"{"status":"current","year":2}"#).await;
    student(&h, "2", r#"{"status":"graduated","year":3}"#).await;
    let mut s = NewStaff::new(
        "english@sealbay.example",
        "Test",
        "kelp forest at dawn",
        Locale::en(),
    );
    s.roles = vec!["Officer".into()];
    h.create_staff_with(s).await;
    let b = app.browser();
    let r = b
        .login("english@sealbay.example", "kelp forest at dawn")
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let list = b.get("/admin/students").await.body;
    assert!(list.contains("Studying"), "{list}");
    // `graduated` has no English label: the Thai one stands in.
    assert!(list.contains("จบการศึกษา"), "{list}");
}

fn edit_form(year: &str, status: &str) -> String {
    format!(
        "display_name=Nok+1&email={}&locale=th&attr.status={status}&attr.year={}",
        urlencode("nok1@student.sealbay.example"),
        urlencode(year)
    )
}

#[tokio::test]
async fn the_edit_form_refuses_a_year_outside_the_range_and_accepts_the_edge() {
    let (h, app) = boot().await;
    student(&h, "1", r#"{"status":"current","year":2}"#).await;
    let b = signed_in(&h, &app, "Officer").await;
    let version = |body: &str| {
        let key = "name=\"version\" value=\"";
        let at = body.find(key).expect("a version field") + key.len();
        body[at..at + 64].to_string()
    };
    let v = version(&b.get("/admin/students/1/edit").await.body);
    for bad in ["7", "0", "-1"] {
        let r = b
            .post(
                "/admin/students/1/edit",
                &format!("{}&version={v}", edit_form(bad, "current")),
            )
            .await;
        assert_eq!(r.status, 422, "{bad}");
        assert!(r.body.contains("ค่าอยู่นอกช่วงที่กำหนด"), "{bad}: {}", r.body);
    }
    let year = || async {
        let rows = app
            .query("SELECT json_extract(attributes, '$.year') AS y FROM student WHERE id = '1'")
            .await;
        rows.get(0, "y").and_then(|v| v.as_i64())
    };
    assert_eq!(year().await, Some(2), "nothing was written");
    let r = b
        .post(
            "/admin/students/1/edit",
            &format!("{}&version={v}", edit_form("6", "graduated")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(year().await, Some(6));
}
