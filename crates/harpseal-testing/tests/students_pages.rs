//! Students pages (#22) over HTTP: list and pagination, HTMX search,
//! the detail page and its read audit, Permissions and CSRF. Edits, Tags,
//! status and Links: `students_edit.rs`.

mod students_support;
mod support;

use harpseal_testing::urlencode;
use students_support::{boot, htmx_get, one, rows_shown, run, signed_in};
use support::Chat;

/// 120 more Students: `Wave 001` .. `Wave 120`, codes 69000001.. .
async fn seed_120(h: &harpseal_testing::Harness) {
    for i in 1..=120 {
        run(
            h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, email, locale, status, \
                 attributes, created_at, updated_at) VALUES ('w{i}', '6900{i:04}', 'Wave {i:03}', \
                 NULL, 'th', 'active', '{{\"year\":1,\"programme\":\"DDS-Intl\"}}', 1, 1)"
            ),
        )
        .await;
    }
}

#[tokio::test]
async fn the_list_shows_50_students_per_page_with_a_next_page() {
    let (h, app) = boot().await;
    seed_120(&h).await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let r = b.get("/admin/students").await;
    assert_eq!(r.status, 200);
    assert_eq!(rows_shown(&r.body), 50, "{}", r.body);
    assert!(r.body.contains("ทั้งหมด 124"), "124 Students in all");
    assert!(
        r.body
            .contains("rel=\"next\" href=\"/admin/students?q=&amp;page=2\"")
    );
    assert!(!r.body.contains("rel=\"prev\""));
    // Cards below 900 px / table from 900 px: one `table.rows`, labelled
    // cells, the listed attributes in schema order, one icon per Channel.
    assert!(r.body.contains("<table class=\"rows students\">"));
    let programme = r.body.find("<th>หลักสูตร</th>").unwrap();
    let year = r.body.find("<th>ชั้นปี</th>").unwrap();
    assert!(programme < year, "order 1 before order 2");
    assert!(!r.body.contains("วันเข้าศึกษา"), "not shown_in_list");
    assert!(r.body.contains("data-label=\"ชั้นปี\">2</td>"));
    assert!(r.body.contains("aria-label=\"fake: ยังไม่เชื่อม\""));
    // The HTMX search box.
    assert!(r.body.contains("hx-get=\"/admin/students\""));
    assert!(
        r.body
            .contains("hx-trigger=\"input changed delay:250ms, search\"")
    );

    let last = b.get("/admin/students?page=3").await;
    assert_eq!(rows_shown(&last.body), 24);
    assert!(last.body.contains("rel=\"prev\""));
    assert!(!last.body.contains("rel=\"next\""));
    assert!(last.body.contains("Wave 120"));
    let bad = b.get("/admin/students?page=x").await;
    assert_eq!(rows_shown(&bad.body), 50, "a bad page number is page 1");
}

#[tokio::test]
async fn search_by_partial_name_or_code_returns_the_fragment() {
    let (h, app) = boot().await;
    seed_120(&h).await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let r = htmx_get(&b, &format!("/admin/students?q={}", urlencode("ok Tha"))).await;
    assert_eq!(r.status, 200);
    assert!(
        r.body.starts_with("<div id=\"student-results\">"),
        "{}",
        r.body
    );
    assert!(!r.body.contains("<html"), "only the fragment");
    assert_eq!(rows_shown(&r.body), 1);
    assert!(r.body.contains("Nok Thale"));

    let by_code = htmx_get(&b, "/admin/students?q=68010004").await;
    assert_eq!(rows_shown(&by_code.body), 1);
    assert!(by_code.body.contains("Karn Bai-ruea"));
    let partial_code = htmx_get(&b, "/admin/students?q=690001").await;
    assert_eq!(rows_shown(&partial_code.body), 21, "69000100..69000120");

    // `%` and `_` are literal, not wildcards.
    let wild = htmx_get(&b, &format!("/admin/students?q={}", urlencode("%"))).await;
    assert_eq!(rows_shown(&wild.body), 0);
    assert!(wild.body.contains("ไม่พบนักศึกษา"));
    // Without HTMX the whole page comes back, search box filled in.
    let full = b.get("/admin/students?q=Wave+11").await;
    assert!(full.body.contains("<html"));
    assert!(full.body.contains("value=\"Wave 11\""));
    assert_eq!(rows_shown(&full.body), 10, "Wave 110..Wave 119");
}

#[tokio::test]
async fn the_detail_shows_links_tags_consent_and_logs_the_read() {
    let (h, app) = boot().await;
    let mut c = Chat::new(&h, &app, "u1");
    c.link("68010001").await;
    for sql in [
        "INSERT INTO tag (id, name, created_at) VALUES ('t1', 'DDS-1', 1), ('t2', 'ทุนเรียนดี', 1)",
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) VALUES \
         ('s1', 't1', 'roster', 1), ('s1', 't2', 'manual', 1)",
        "INSERT INTO security_event (id, at, kind, student_id) VALUES \
         ('e1', 1793836800000, 'otp.lockout', 's1')",
    ] {
        run(&h, sql).await;
    }
    let (b, staff_id) = signed_in(&h, &app, "Viewer").await;

    let r = b.get("/admin/students/s1").await;
    assert_eq!(r.status, 200);
    let body = &r.body;
    assert!(body.contains("Nok Thale") && body.contains("68010001"));
    assert!(body.contains("s68010001@student.sealbay.example"));
    assert!(body.contains("<dt>หลักสูตร</dt><dd>DDS</dd>"), "{body}");
    assert!(body.contains("<dt>ชั้นปี</dt><dd>2</dd>"));
    assert!(body.contains("<dt>วันเข้าศึกษา</dt><dd>ไม่มีข้อมูล</dd>"));
    // Links per Channel with status, method and time.
    assert!(body.contains("aria-label=\"fake: เชื่อมอยู่\""));
    assert!(body.contains("ยืนยันทางอีเมล"));
    assert!(body.contains("รหัสบัญชี u1"));
    // Tags grouped roster vs manual; Consent; the Security Event.
    let roster = body.find("จากทะเบียน").unwrap();
    let manual = body.find("เพิ่มโดยเจ้าหน้าที่").unwrap();
    let dds = body.find(">DDS-1<").unwrap();
    let scholarship = body.find(">ทุนเรียนดี<").unwrap();
    assert!(roster < dds && dds < manual && manual < scholarship);
    assert!(body.contains("ฉบับที่ 1"));
    assert!(body.contains("otp.lockout") && body.contains("2026-11-05 07:00"));
    // A Viewer sees no edit, Tag, status, Manual Link or revoke forms.
    for hidden in ["/edit\"", "/tags\"", "/status\"", "/links\"", "/revoke\""] {
        assert!(!body.contains(hidden), "{hidden}");
    }

    let rows = app
        .query(
            "SELECT actor_kind, actor_id, target_kind, target_id, before, after, request_id \
             FROM audit_log WHERE action = 'student.view'",
        )
        .await;
    assert_eq!(rows.len(), 1);
    let col = |c: &str| rows.get(0, c).and_then(|v| v.as_text()).map(str::to_string);
    assert_eq!(col("actor_kind").as_deref(), Some("staff"));
    assert_eq!(col("actor_id"), Some(staff_id));
    assert_eq!(col("target_kind").as_deref(), Some("student"));
    assert_eq!(col("target_id").as_deref(), Some("s1"));
    assert_eq!(col("before"), None);
    assert_eq!(col("after"), None);
    assert_eq!(col("request_id").as_deref(), r.header("x-request-id"));

    assert_eq!(b.get("/admin/students/nobody").await.status, 404);
}

#[tokio::test]
async fn a_viewer_gets_403_on_edit_and_manual_link() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;
    assert_eq!(b.get("/admin/students").await.status, 200);
    assert_eq!(b.get("/admin/students/s1/edit").await.status, 403);
    let posts = [
        ("/admin/students/s1/edit", "display_name=X&email=&locale=th"),
        ("/admin/students/s1/tags", "name=X"),
        ("/admin/students/s1/status", "status=left&confirm=yes"),
        ("/admin/students/s1/links", "channel=fake&external_id=u1"),
        ("/admin/students/s1/links/l1/revoke", "confirm=yes"),
    ];
    for (path, form) in posts {
        let r = b.post(path, form).await;
        assert_eq!(r.status, 403, "{path}");
    }
    assert_eq!(
        one(
            &app,
            "SELECT display_name FROM student WHERE id = 's1'",
            "display_name"
        )
        .await
        .as_deref(),
        Some("Nok Thale")
    );
    let writes = app
        .count("SELECT count(*) FROM audit_log WHERE action LIKE 'student.%'")
        .await;
    assert_eq!(writes, 0);
    // The Students page is in the Viewer's navigation.
    assert!(
        b.get("/admin")
            .await
            .body
            .contains("href=\"/admin/students\"")
    );
}

#[tokio::test]
async fn a_post_without_the_csrf_token_is_refused_and_writes_nothing() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let r = b
        .post_raw(
            "/admin/students/s1/edit",
            "display_name=Hacked&email=&locale=th&attr.year=2&attr.programme=DDS",
        )
        .await;
    assert_eq!(r.status, 403);
    let r = b
        .post_raw("/admin/students/s1/status", "status=left&confirm=yes")
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(
        one(
            &app,
            "SELECT display_name, status FROM student WHERE id = 's1'",
            "status"
        )
        .await
        .as_deref(),
        Some("active")
    );
    assert_eq!(
        app.count("SELECT count(*) FROM student WHERE display_name = 'Hacked'")
            .await,
        0
    );
}
