//! The Students list marks and filters Students with missing data (#71):
//! no email, an email that is not valid, or a declared attribute with no
//! value. Fictional Students of Seal Bay University.

mod students_support;
mod support;

use students_support::{boot, htmx_get, rows_shown, run, signed_in};

/// Complete: a valid email and all three attributes.
const FULL: &str = r#"{"year":2,"programme":"MSC","entered":"2025-06-01"}"#;

async fn add(
    h: &harpseal_testing::Harness,
    id: &str,
    name: &str,
    email: Option<&str>,
    attrs: &str,
) {
    let email = email.map_or("NULL".to_string(), |e| format!("'{e}'"));
    run(
        h,
        &format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             attributes, created_at, updated_at) VALUES ('{id}', '{id}', '{name}', {email}, \
             'th', 'active', '{attrs}', 1, 1)"
        ),
    )
    .await;
}

/// The four seeded Students (s1..s4) are not complete: give them everything,
/// so only the Students a test adds can be missing something.
async fn complete(h: &harpseal_testing::Harness) {
    run(h, &format!("UPDATE student SET attributes = '{FULL}'")).await;
    run(
        h,
        "UPDATE student SET email = 's' || id || '@student.sealbay.example'",
    )
    .await;
}

#[tokio::test]
async fn the_filter_lists_students_with_no_email_a_bad_email_or_an_empty_attribute() {
    let (h, app) = boot().await;
    complete(&h).await;
    let ok = Some("fine@student.sealbay.example");
    add(&h, "m-ok", "Complete", ok, FULL).await;
    add(&h, "m-none", "NoEmail", None, FULL).await;
    add(&h, "m-blank", "BlankEmail", Some("  "), FULL).await;
    add(&h, "m-bad", "BadEmail", Some("nok@sealbay"), FULL).await;
    add(&h, "m-bad2", "BadEmailToo", Some("a b@x.example"), FULL).await;
    add(
        &h,
        "m-opt",
        "NoEntryDate",
        ok,
        r#"{"year":2,"programme":"MSC"}"#,
    )
    .await;
    add(
        &h,
        "m-null",
        "NullYear",
        ok,
        r#"{"year":null,"programme":"MSC","entered":"2025-06-01"}"#,
    )
    .await;
    add(
        &h,
        "m-text",
        "BlankText",
        ok,
        r#"{"year":2,"programme":" ","entered":"2025-06-01"}"#,
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let all = b.get("/admin/students").await;
    assert!(all.body.contains("ทั้งหมด 12"), "4 + 8 Students");
    assert_eq!(
        all.body.matches("data-missing=").count(),
        7,
        "a badge on each"
    );

    let r = b.get("/admin/students?missing=1").await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ทั้งหมด 7"), "{}", r.body);
    assert_eq!(rows_shown(&r.body), 7);
    for gone in ["Complete", "fine@"] {
        assert!(!r.body.contains(&format!(">{gone}<")), "{gone}");
    }
    for name in [
        "NoEmail",
        "BlankEmail",
        "BadEmail",
        "BadEmailToo",
        "NoEntryDate",
        "NullYear",
        "BlankText",
    ] {
        assert!(r.body.contains(&format!(">{name}</a>")), "{name} is listed");
    }
    // The badge says what is missing, in the Staff member's words.
    assert!(r.body.contains("ข้อมูลไม่ครบ"));
    assert!(r.body.contains("ไม่มีอีเมล"));
    assert!(r.body.contains("อีเมลไม่ถูกต้อง"));
    assert!(r.body.contains("วันเข้าศึกษา"), "the attribute's label");
    // The checkbox is on, and the pager keeps the filter.
    assert!(
        r.body.contains("name=\"missing\" value=\"1\" checked"),
        "{}",
        r.body
    );
}

#[tokio::test]
async fn the_filter_works_with_search_pages_and_htmx() {
    let (h, app) = boot().await;
    complete(&h).await;
    for i in 1..=60 {
        add(&h, &format!("p{i:03}"), &format!("Wave {i:03}"), None, FULL).await;
    }
    add(
        &h,
        "q001",
        "Plain Complete",
        Some("c@student.sealbay.example"),
        FULL,
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let r = b.get("/admin/students?missing=1").await;
    assert!(r.body.contains("ทั้งหมด 60"), "the 5 complete ones are out");
    assert_eq!(rows_shown(&r.body), 50);
    assert!(
        r.body
            .contains("href=\"/admin/students?q=&amp;missing=1&amp;page=2\""),
        "{}",
        r.body
    );
    let p2 = b.get("/admin/students?missing=1&page=2").await;
    assert_eq!(rows_shown(&p2.body), 10);

    // Search narrows the filtered list; without the filter it is wider.
    let s = htmx_get(&b, "/admin/students?q=Wave+05&missing=1").await;
    assert!(
        s.body.starts_with("<div id=\"student-results\""),
        "a fragment"
    );
    assert_eq!(rows_shown(&s.body), 10, "Wave 050..059");
    let none = htmx_get(&b, "/admin/students?q=Plain&missing=1").await;
    assert_eq!(rows_shown(&none.body), 0);
    let some = htmx_get(&b, "/admin/students?q=Plain").await;
    assert_eq!(rows_shown(&some.body), 1);
    assert!(
        !some.body.contains("data-missing="),
        "a complete Student has no badge"
    );
}
