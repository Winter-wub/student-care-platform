//! The first-run checklist on the home (#127): set the branding, import
//! the roster, add Staff, send the first announcement.
//!
//! It shows only while the Deployment is fresh, by one rule: **no Student
//! yet**. An established Deployment (any Student, from a roster import or
//! added by hand) never sees it. While it shows, each step ticks itself
//! from data already kept (no table of its own): branding set in
//! `[branding]` or saved on `/admin/branding`, a second Staff account, an
//! announcement sending or sent. A step the Staff member has no
//! Permission for is not drawn, nor the roster step where `[roster]` maps
//! no file. One query for all of it; when it fails, the block says so, as
//! a status card does.

use maud::{Markup, html};

use crate::auth::staff_admin;
use crate::config::{BrandingConfig, DEFAULT_PRIMARY};
use crate::kernel::{branding, roster};
use crate::modules::announcements::SEND_PERMISSION;
use crate::port_types::{Statement, Value};
use crate::{AppState, CurrentStaff};

/// One step: its text, where it is done, who may do it, and when it is
/// done: an SQL expression (one row, no table) that is 1, or already
/// known.
struct Step {
    key: &'static str,
    href: &'static str,
    permission: &'static str,
    done: Done,
}

enum Done {
    Known(bool),
    Sql(String),
}

/// Whether `[branding]` in the TOML differs from the default (a colour in
/// either case).
fn configured(b: &BrandingConfig) -> bool {
    b.name.is_some() || b.logo.is_some() || !b.primary_color.eq_ignore_ascii_case(DEFAULT_PRIMARY)
}

/// The steps `staff` may see on this Deployment, in order.
fn steps(s: &AppState, staff: &CurrentStaff) -> Vec<Step> {
    let saved = format!(
        "EXISTS (SELECT 1 FROM setting WHERE locale = '' AND key IN ('{}', '{}', '{}'))",
        branding::NAME_KEY,
        branding::COLOR_KEY,
        branding::LOGO_KEY
    );
    let brand = if configured(&s.config.branding) {
        Done::Known(true)
    } else {
        Done::Sql(saved)
    };
    let all = [
        Step {
            key: "kernel.setup.branding",
            href: "/admin/branding",
            permission: branding::EDIT,
            done: brand,
        },
        // Shown only while there is no Student: never done here.
        Step {
            key: "kernel.setup.roster",
            href: "/admin/roster",
            permission: roster::IMPORT,
            done: Done::Known(false),
        },
        Step {
            key: "kernel.setup.staff",
            href: "/admin/staff",
            permission: staff_admin::MANAGE,
            done: Done::Sql("(SELECT COUNT(*) FROM staff) > 1".into()),
        },
        Step {
            key: "kernel.setup.announce",
            href: "/admin/announcements/new",
            permission: SEND_PERMISSION,
            done: Done::Sql(
                "EXISTS (SELECT 1 FROM announcement WHERE status IN ('sending', 'sent'))".into(),
            ),
        },
    ];
    let mapped = roster::has_mapping(&s.config.roster);
    // The announcement table exists only where the Module is mounted.
    let announcements = s.deployment.has_back_office("/announcements/new");
    all.into_iter()
        .filter(|st| staff.can(st.permission))
        .filter(|st| st.key != "kernel.setup.roster" || mapped)
        .filter(|st| st.key != "kernel.setup.announce" || announcements)
        .collect()
}

/// What the block shows.
enum Shown {
    /// Not fresh, or no step to show.
    Nothing,
    /// The steps, each with whether it is done.
    Steps(Vec<(Step, bool)>),
    /// The query failed.
    Unreadable,
}

async fn read(s: &AppState, staff: &CurrentStaff) -> Shown {
    let steps = steps(s, staff);
    if steps.is_empty() {
        return Shown::Nothing;
    }
    let mut cols = vec!["EXISTS (SELECT 1 FROM student) AS established".to_string()];
    for (i, st) in steps.iter().enumerate() {
        if let Done::Sql(sql) = &st.done {
            cols.push(format!("{sql} AS s{i}"));
        }
    }
    let q = Statement::new(format!("SELECT {}", cols.join(", ")));
    let Ok(rows) = s.services.db.query(q).await else {
        return Shown::Unreadable;
    };
    let one = |c: &str| rows.get(0, c).and_then(Value::as_i64) == Some(1);
    if one("established") {
        return Shown::Nothing;
    }
    let steps = steps
        .into_iter()
        .enumerate()
        .map(|(i, st)| {
            let done = match &st.done {
                Done::Known(d) => *d,
                Done::Sql(_) => one(&format!("s{i}")),
            };
            (st, done)
        })
        .collect();
    Shown::Steps(steps)
}

/// The checklist, or nothing once the Deployment is established or every
/// step drawn is done.
pub(super) async fn block(s: &AppState, staff: &CurrentStaff) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let steps = match read(s, staff).await {
        Shown::Nothing => return html! {},
        Shown::Unreadable => {
            return html! {
                section class="card setup" id="setup" aria-labelledby="setup-title" data-failed {
                    h2 id="setup-title" { (t("kernel.setup.title")) }
                    p { (t("kernel.home.unreadable")) }
                }
            };
        }
        Shown::Steps(steps) => steps,
    };
    let left = steps.iter().filter(|(_, done)| !done).count();
    if left == 0 {
        return html! {};
    }
    let (left, total) = (left.to_string(), steps.len().to_string());
    let intro = s.catalog.tf(
        &staff.locale,
        "kernel.setup.intro",
        &[("left", &left), ("total", &total)],
    );
    html! {
        section class="card setup" id="setup" aria-labelledby="setup-title" {
            h2 id="setup-title" { (t("kernel.setup.title")) }
            p class="muted" { (intro) }
            ol class="list checklist" {
                @for (st, done) in &steps {
                    li data-done[*done] {
                        span class=(if *done { "tick done" } else { "tick" }) aria-hidden="true" {}
                        a class="name" href=(st.href) { (t(st.key)) }
                        @if *done {
                            span class="chip ok" { (t("kernel.setup.done")) }
                        } @else {
                            span class="chip" { (t("kernel.setup.todo")) }
                        }
                    }
                }
            }
        }
    }
}
