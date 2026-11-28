//! An attribute with a condition is "missing data" only for the Students it
//! applies to (#71, #180): a cohort is missing for a graduate, not for a
//! current Student; a year is not missing for a graduate. The badge and the
//! filter follow the same rule. Fictional Students of Seal Bay University.

mod students_support;
mod support;

use harpseal::{DeploymentConfig, Statement};
use harpseal_testing::{Harness, TestApp};
use students_support::{htmx_get, rows_shown, signed_in};

const CONFIG: &str = r#"
name = "Seal Bay University (cohorts)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Viewer = ["kernel.student.view"]

[roster.attributes.standing]
column = "สถานะ"
type = "enum"
values = ["current", "graduated"]
label = { th = "สถานะ", en = "Standing" }
order = 1

# Required, except for a graduate.
[roster.attributes.year]
column = "ชั้นปี"
type = "integer"
optional_when = { standing = ["graduated"] }
label = { th = "ชั้นปี", en = "Year" }
order = 2

# May be empty, except for a graduate.
[roster.attributes.cohort]
column = "รุ่นศิษย์เก่า"
type = "integer"
optional = true
required_when = { standing = ["graduated"] }
label = { th = "รุ่นศิษย์เก่า", en = "Alumni cohort" }
order = 3

# Plain optional: missing whenever empty (unchanged since #71).
[roster.attributes.note]
column = "หมายเหตุ"
type = "text"
optional = true
label = { th = "หมายเหตุ", en = "Note" }
order = 4
"#;

async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    (h, app)
}

async fn add(h: &Harness, id: &str, attrs: &str) {
    h.db()
        .execute(Statement::new(format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             attributes, created_at, updated_at) VALUES ('{id}', '{id}', '{id}', \
             '{id}@student.sealbay.example', 'th', 'active', '{attrs}', 1, 1)"
        )))
        .await
        .unwrap();
}

const NOTE: &str = r#""note":"x""#;

#[tokio::test]
async fn a_current_student_without_a_cohort_is_complete() {
    let (h, app) = boot().await;
    add(
        &h,
        "cur-ok",
        &format!(r#"{{"standing":"current","year":2,{NOTE}}}"#),
    )
    .await;
    add(
        &h,
        "cur-null",
        &format!(r#"{{"standing":"current","year":2,"cohort":null,{NOTE}}}"#),
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let all = b.get("/admin/students").await;
    assert_eq!(rows_shown(&all.body), 2);
    assert!(!all.body.contains("data-missing="), "{}", all.body);
    assert!(!all.body.contains("รุ่นศิษย์เก่า</"), "no cohort chip");
    let r = b.get("/admin/students?missing=1").await;
    assert_eq!(rows_shown(&r.body), 0, "the filter agrees");
}

#[tokio::test]
async fn an_alumnus_without_a_cohort_is_incomplete() {
    let (h, app) = boot().await;
    add(
        &h,
        "alum-ok",
        &format!(r#"{{"standing":"graduated","cohort":2,{NOTE}}}"#),
    )
    .await;
    add(
        &h,
        "alum-none",
        &format!(r#"{{"standing":"graduated",{NOTE}}}"#),
    )
    .await;
    add(
        &h,
        "alum-blank",
        &format!(r#"{{"standing":"graduated","cohort":null,{NOTE}}}"#),
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let all = b.get("/admin/students").await;
    assert_eq!(rows_shown(&all.body), 3);
    assert_eq!(all.body.matches("data-missing=").count(), 2, "{}", all.body);
    assert!(all.body.contains("รุ่นศิษย์เก่า"));

    let r = htmx_get(&b, "/admin/students?missing=1").await;
    assert_eq!(rows_shown(&r.body), 2);
    assert!(r.body.contains(">alum-none</a>") && r.body.contains(">alum-blank</a>"));
    assert!(!r.body.contains(">alum-ok</a>"));
}

#[tokio::test]
async fn the_year_is_not_missing_for_an_alumnus_but_is_for_a_current_student() {
    let (h, app) = boot().await;
    add(
        &h,
        "alum-noyear",
        &format!(r#"{{"standing":"graduated","cohort":1,{NOTE}}}"#),
    )
    .await;
    add(
        &h,
        "cur-noyear",
        &format!(r#"{{"standing":"current",{NOTE}}}"#),
    )
    .await;
    // No standing at all: nothing makes the year optional, the cohort is not required.
    add(&h, "no-standing", &format!(r#"{{"year":1,{NOTE}}}"#)).await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let r = htmx_get(&b, "/admin/students?missing=1").await;
    assert!(r.body.contains(">cur-noyear</a>"), "{}", r.body);
    assert!(!r.body.contains(">alum-noyear</a>"));
    // The standing itself is a plain required attribute: missing when empty.
    assert!(r.body.contains(">no-standing</a>"));
    assert_eq!(rows_shown(&r.body), 2);
    assert_eq!(
        b.get("/admin/students")
            .await
            .body
            .matches("data-missing=")
            .count(),
        2,
        "the badge agrees with the filter"
    );
}

#[tokio::test]
async fn an_attribute_without_a_condition_is_missing_when_empty_as_before() {
    let (h, app) = boot().await;
    add(&h, "plain", r#"{"standing":"current","year":2}"#).await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;
    let r = htmx_get(&b, "/admin/students?missing=1").await;
    assert_eq!(rows_shown(&r.body), 1);
    assert!(r.body.contains("หมายเหตุ"), "{}", r.body);
}
