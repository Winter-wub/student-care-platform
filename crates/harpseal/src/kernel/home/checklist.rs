//! The first-run checklist on the home (#127): set the branding, import
//! the roster, add Staff, send the first announcement. Each step ticks
//! itself from data already kept (no table of its own): a saved branding
//! setting, any Student, a second Staff account, an announcement past its
//! draft. A step the Staff member has no Permission for is not drawn; the
//! block goes away once every drawn step is done. One query for all of it.

use maud::{Markup, html};

use crate::kernel::branding;
use crate::port_types::{Statement, Value};
use crate::{AppState, CurrentStaff};

/// One step: its text, where it is done, who may do it, and the SQL
/// expression (one row, no table) that is 1 when it is done.
struct Step {
    key: &'static str,
    href: &'static str,
    permission: &'static str,
    done_sql: &'static str,
}

const STEPS: [Step; 4] = [
    Step {
        key: "kernel.setup.branding",
        href: "/admin/branding",
        permission: branding::EDIT,
        done_sql: "EXISTS (SELECT 1 FROM setting WHERE locale = '' AND key IN \
                   ('kernel.branding.name', 'kernel.branding.primary_color', \
                   'kernel.branding.logo_file_key'))",
    },
    Step {
        key: "kernel.setup.roster",
        href: "/admin/roster",
        permission: "kernel.roster.import",
        done_sql: "EXISTS (SELECT 1 FROM student)",
    },
    Step {
        key: "kernel.setup.staff",
        href: "/admin/staff",
        permission: "kernel.staff.manage",
        done_sql: "(SELECT COUNT(*) FROM staff) > 1",
    },
    Step {
        key: "kernel.setup.announce",
        href: "/admin/announcements/new",
        permission: crate::modules::announcements::SEND_PERMISSION,
        done_sql: "EXISTS (SELECT 1 FROM announcement WHERE status != 'draft')",
    },
];

/// The steps `staff` may take, each with whether it is done; empty when
/// none is drawn or the query failed (the home renders without it).
async fn steps(s: &AppState, staff: &CurrentStaff) -> Vec<(&'static Step, bool)> {
    // The announcements step only where the Module is mounted (its table
    // exists only then).
    let shown: Vec<&Step> = STEPS
        .iter()
        .filter(|st| staff.can(st.permission))
        .filter(|st| {
            st.key != "kernel.setup.announce" || s.deployment.has_back_office("/announcements/new")
        })
        .collect();
    if shown.is_empty() {
        return Vec::new();
    }
    let cols: Vec<String> = shown
        .iter()
        .enumerate()
        .map(|(i, st)| format!("{} AS s{i}", st.done_sql))
        .collect();
    let q = Statement::new(format!("SELECT {}", cols.join(", ")));
    let Ok(rows) = s.services.db.query(q).await else {
        return Vec::new();
    };
    shown
        .into_iter()
        .enumerate()
        .map(|(i, st)| {
            let done = rows.get(0, &format!("s{i}")).and_then(Value::as_i64) == Some(1);
            (st, done)
        })
        .collect()
}

/// The checklist, or nothing once every step drawn is done.
pub(super) async fn block(s: &AppState, staff: &CurrentStaff) -> Markup {
    let steps = steps(s, staff).await;
    let left = steps.iter().filter(|(_, done)| !done).count();
    if left == 0 {
        return html! {};
    }
    let t = |k: &str| s.catalog.t(&staff.locale, k);
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
