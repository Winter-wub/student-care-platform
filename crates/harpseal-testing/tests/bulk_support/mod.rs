//! Setup for the bulk action tests (#145): a Deployment with the
//! Announcements Module, Roles holding each bulk Permission alone, the
//! chat-flow roster (s1..s4, s2 left), and helpers to read the review.

#![allow(dead_code)]

use harpseal::DeploymentConfig;
use harpseal::modules::announcements::Announcements;
use harpseal_testing::{Browser, Harness, TestApp, TestResponse};

use crate::students_support::run;
use crate::support::ROSTER;

pub const CONFIG: &str = r#"
name = "Seal Bay University (bulk)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Everything = ["kernel.student.view", "kernel.student.edit", "kernel.link.revoke", "announcements.send"]
Tagger = ["kernel.student.view", "kernel.student.edit"]
Viewer = ["kernel.student.view"]
Sender = ["kernel.student.view", "announcements.send"]
SendOnly = ["announcements.send"]
"#;

pub const BULK: &str = "/admin/students/bulk";

pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let app = h
        .boot(
            h.builder()
                .config(config)
                .module(Announcements)
                .build()
                .unwrap(),
        )
        .await;
    for sql in ROSTER {
        run(&h, sql).await;
    }
    (h, app)
}

/// The value of hidden field `name` in a page.
pub fn hidden(body: &str, name: &str) -> String {
    let key = format!("name=\"{name}\" value=\"");
    let at = body.find(&key).unwrap_or_else(|| panic!("no {name}")) + key.len();
    body[at..at + body[at..].find('"').unwrap()].to_string()
}

/// The fields a review's confirm posts beyond the selection: `n`, `set`
/// and `confirm=yes`.
pub fn confirm_of(review: &TestResponse) -> String {
    format!(
        "confirm=yes&n={}&set={}",
        hidden(&review.body, "n"),
        hidden(&review.body, "set")
    )
}

/// Reviews `form`, then confirms it with `extra` (`&typed=…`).
pub async fn review_then_confirm(b: &Browser<'_>, form: &str, extra: &str) -> TestResponse {
    let review = b.post(BULK, form).await;
    assert_eq!(review.status, 200, "{}", review.body);
    b.post(BULK, &format!("{form}&{}{extra}", confirm_of(&review)))
        .await
}

/// `n` more active Students `x0`.. with codes `6902xxxx` (after the
/// roster's), no email.
pub async fn many(h: &Harness, n: usize) {
    let values: Vec<String> = (0..n)
        .map(|i| format!("('x{i}', '6902{i:04}', 'Krathing {i}', NULL, 'th', 'active', 1, 1)"))
        .collect();
    run(
        h,
        &format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             created_at, updated_at) VALUES {}",
            values.join(", ")
        ),
    )
    .await;
}

/// A live Link on the Fake Channel for Student `id`, written directly.
pub async fn link_row(h: &Harness, id: &str) {
    run(
        h,
        &format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('ca-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
        ),
    )
    .await;
    run(
        h,
        &format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('l-{id}', '{id}', 'ca-{id}', 'fake', 'active', \
             'email_otp', 1, 1)"
        ),
    )
    .await;
}
