//! Audit log pages (#27) over HTTP: filters by action prefix and actor,
//! the redacted before/after diff, one read row per request.

mod admin_support;
mod support;

use admin_support::{audits, boot, email_of, hidden, htmx_get, rows, run, signed_in};
use harpseal_testing::urlencode;

/// The officer renames s1 through the Students edit form.
async fn rename(b: &harpseal_testing::Browser<'_>, name: &str) {
    let form = b.get("/admin/students/s1/edit").await.body;
    let version = hidden(&form, "version");
    let body = format!(
        "display_name={}&email={}&locale=th&version={version}",
        urlencode(name),
        urlencode("s68010001@student.sealbay.example")
    );
    let r = b.post("/admin/students/s1/edit", &body).await;
    assert_eq!(r.status, 303, "{}", r.body);
}

#[tokio::test]
async fn the_log_filters_by_action_prefix_and_actor() {
    let (h, app) = boot().await;
    let (officer, officer_id) = signed_in(&h, &app, "Officer").await;
    rename(&officer, "Nok Thale-Sai").await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;

    let all = b.get("/admin/audit").await;
    assert_eq!(all.status, 200, "{}", all.body);
    assert!(all.body.contains("student.update"));
    assert!(all.body.contains("staff.login"));
    assert!(
        all.body.contains("href=\"/admin/students/s1\""),
        "target linked"
    );
    assert!(
        all.body
            .contains(&format!("href=\"/admin/staff/{officer_id}\"")),
        "actor shown by name, linked"
    );

    let students = b.get("/admin/audit?action=student.").await;
    assert_eq!(rows(&students.body), 1, "{}", students.body);
    assert!(students.body.contains("student.update"));
    assert!(!students.body.contains("staff.login"));
    // `_` and `%` are literal in the prefix.
    assert_eq!(rows(&b.get("/admin/audit?action=student_").await.body), 0);

    let by_officer = b
        .get(&format!(
            "/admin/audit?actor={}",
            urlencode(&email_of("Officer"))
        ))
        .await;
    assert!(by_officer.body.contains("student.update"));
    assert!(
        !by_officer.body.contains("audit.view"),
        "the viewer's reads excluded"
    );
    let by_kind = b.get("/admin/audit?actor=system").await;
    assert!(
        by_kind.body.contains("staff.create"),
        "harness creates Staff as system"
    );
    assert!(!by_kind.body.contains("student.update"));
    let by_target = b.get("/admin/audit?target=student%3As1").await;
    assert_eq!(rows(&by_target.body), 1);
    assert_eq!(rows(&b.get("/admin/audit?from=2026-11-06").await.body), 0);

    let frag = htmx_get(&b, "/admin/audit?action=student.").await;
    assert!(frag.body.starts_with("<div id=\"audit-results\""));
    assert_eq!(frag.header("vary"), Some("HX-Request"));
}

#[tokio::test]
async fn one_read_row_per_request_however_many_rows_are_listed() {
    let (h, app) = boot().await;
    for i in 0..60 {
        run(
            &h,
            &format!(
                "INSERT INTO audit_log (id, at, actor_kind, action) \
                 VALUES ('seed-{i:02}', {i}, 'system', 'demo.seed')"
            ),
        )
        .await;
    }
    let (b, _) = signed_in(&h, &app, "Viewer").await;
    let first = b.get("/admin/audit?action=demo.").await;
    assert_eq!(rows(&first.body), 50);
    assert!(first.body.contains("rel=\"next\""));
    assert_eq!(audits(&h, "audit.view").await, 1);
    let second = b.get("/admin/audit?action=demo.&page=2").await;
    assert_eq!(rows(&second.body), 10);
    assert_eq!(audits(&h, "audit.view").await, 2);
}

#[tokio::test]
async fn the_detail_diffs_before_and_after_and_never_shows_a_secret() {
    let (h, app) = boot().await;
    let (officer, _) = signed_in(&h, &app, "Officer").await;
    rename(&officer, "Nok Thale-Sai").await;
    // A careless writer stored a hash: the page still redacts it.
    run(
        &h,
        "INSERT INTO audit_log (id, at, actor_kind, action, target_kind, target_id, before, after) \
         VALUES ('leak', 2, 'system', 'staff.demo', 'staff', 'x', \
         '{\"password_hash\":\"$argon2id$v=19$secret\",\"note\":\"a\"}', \
         '{\"api_token\":\"tok-123\",\"note\":\"b\"}')",
    )
    .await;
    let (b, _) = signed_in(&h, &app, "Viewer").await;
    let id = admin_support::text(
        &h,
        "SELECT id FROM audit_log WHERE action = ?",
        "student.update",
        "id",
    )
    .await
    .unwrap();

    let page = b.get(&format!("/admin/audit/{id}")).await;
    assert_eq!(page.status, 200, "{}", page.body);
    assert!(page.body.contains("Nok Thale-Sai"));
    assert!(page.body.contains("Nok Thale"));
    assert!(
        page.body.contains("<tr class=\"changed\""),
        "changed key highlighted"
    );
    assert!(page.body.contains("display_name"));
    assert_eq!(audits(&h, "audit.detail").await, 1);
    let target = admin_support::text(
        &h,
        "SELECT target_id FROM audit_log WHERE action = ?",
        "audit.detail",
        "target_id",
    )
    .await;
    assert_eq!(target.as_deref(), Some(id.as_str()));

    let leak = b.get("/admin/audit/leak").await;
    assert_eq!(leak.status, 200);
    assert!(leak.body.contains("[redacted]"));
    assert!(!leak.body.contains("argon2id"), "{}", leak.body);
    assert!(!leak.body.contains("tok-123"));
    for (status, path) in [(404, "/admin/audit/nope")] {
        assert_eq!(b.get(path).await.status, status);
    }
}

#[tokio::test]
async fn a_viewer_is_refused_every_manage_and_edit_route() {
    let (h, app) = boot().await;
    let (b, id) = signed_in(&h, &app, "Viewer").await;
    for path in [
        "/admin/staff",
        "/admin/staff/new",
        &format!("/admin/staff/{id}"),
        "/admin/roles",
        "/admin/texts",
        "/admin/texts/kernel.flow.welcome",
    ] {
        assert_eq!(b.get(path).await.status, 403, "GET {path}");
    }
    for path in [
        "/admin/staff/new",
        &format!("/admin/staff/{id}/status"),
        &format!("/admin/staff/{id}/roles"),
        "/admin/roles",
        "/admin/texts/consent",
        "/admin/texts/kernel.flow.welcome",
    ] {
        assert_eq!(b.post(path, "").await.status, 403, "POST {path}");
    }
    assert_eq!(b.get("/admin/security").await.status, 200);
    assert_eq!(b.get("/admin/audit").await.status, 200);
}
