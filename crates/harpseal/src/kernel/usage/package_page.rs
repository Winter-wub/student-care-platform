//! The LINE package on the usage page (#115): the chosen package and its
//! limit, and, for Staff holding `kernel.setting.edit` (the Administrator
//! Role in the example), the form behind `POST /admin/usage/package`.
//!
//! A save is a compare-and-set on the form's token (a stale form is
//! refused with the conflict flash), audited as `setting.update` with the
//! value before and after, in the same batch as the value. The route is
//! guarded by the Permission, and the back-office guard checks the CSRF
//! token of every POST before this handler runs.

use axum::Form;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};
use serde::Deserialize;

use super::package::{self, Choice, Current, KEY, MAX_CUSTOM, PACKAGES, SOURCE_DATE, SOURCE_URL};
use super::store;
use crate::audit::{Actor, RequestCtx};
use crate::kernel::text::fill;
use crate::{AppState, CurrentStaff, PageState};

/// Edits the package; also the form's action (under `/admin`).
pub(crate) const PATH: &str = "/usage/package";
/// The Permission of the form: the one that edits the Deployment's other
/// Kernel settings (texts, the privacy notice version).
pub(crate) const EDIT: &str = "kernel.setting.edit";
/// The form value of "follow `[quotas.line]`": the saved choice is removed.
const CONFIG: &str = "config";
/// Back to the LINE card.
const BACK: &str = "/admin/usage#usage-line";

/// The "current package" block of the LINE card, with the form for Staff
/// who may edit it.
pub(super) fn section(s: &PageState, staff: &CurrentStaff, cur: &Current) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let number = |n: u64| crate::ui::number(n);
    let summary = match cur.choice {
        Choice::Package(p) => fill(
            &t("kernel.usage.package.current"),
            &[("name", p.name), ("limit", &number(p.included))],
        ),
        Choice::Custom(n) => fill(
            &t("kernel.usage.package.current_custom"),
            &[("limit", &number(n))],
        ),
        Choice::Config(Some(n)) => fill(
            &t("kernel.usage.package.current_config"),
            &[("limit", &number(n))],
        ),
        Choice::Config(None) => t("kernel.usage.package.current_none"),
    };
    let extra = match cur.choice {
        Choice::Package(p) if p.extra => Some("kernel.usage.package.extra_yes"),
        Choice::Package(_) => Some("kernel.usage.package.extra_no"),
        _ => None,
    };
    html! {
        div class="package" id="line-package" {
            h3 { (t("kernel.usage.package.title")) }
            p class="package-current" {
                strong { (summary) }
                @if let Some(key) = extra { " · " (t(key)) }
            }
            @if cur.unreadable {
                p class="field-error" { (t("kernel.usage.package.unreadable")) }
            }
            @if staff.can(EDIT) { (form(s, staff, cur)) }
            p class="muted" {
                (t("kernel.usage.package.help")) " "
                (fill(&t("kernel.usage.package.source"), &[("url", SOURCE_URL), ("date", &crate::ui::thai_day(SOURCE_DATE))]))
            }
        }
    }
}

fn form(s: &PageState, staff: &CurrentStaff, cur: &Current) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let chosen = match cur.choice {
        Choice::Package(p) => p.id,
        Choice::Custom(_) => "custom",
        // Nothing saved: following `[quotas.line]`. Its number is also what
        // the Custom field starts from.
        Choice::Config(_) => CONFIG,
    };
    let custom = cur.choice.limit().map(|n| n.to_string());
    html! {
        form method="post" action=(format!("/admin{PATH}")) class="package-form" {
            (staff.csrf_field())
            input type="hidden" name="expected" value=(cur.version());
            div class="field" {
                label for="line-package-choice" { (t("kernel.usage.package.label")) }
                select id="line-package-choice" name="package" {
                    option value=(CONFIG) selected[chosen == CONFIG] {
                        (t("kernel.usage.package.option_config"))
                    }
                    @for p in PACKAGES {
                        @let limit = crate::ui::number(p.included);
                        option value=(p.id) selected[chosen == p.id] {
                            (fill(&t("kernel.usage.package.option"), &[("name", p.name), ("limit", &limit)]))
                        }
                    }
                    option value="custom" selected[chosen == "custom"] {
                        (t("kernel.usage.package.option_custom"))
                    }
                }
            }
            div class="field" {
                label for="line-package-custom" { (t("kernel.usage.package.label_custom")) }
                input id="line-package-custom" name="custom" inputmode="numeric" autocomplete="off"
                    pattern="[0-9]+" maxlength=(MAX_CUSTOM.to_string().len())
                    value=[custom.as_deref()];
            }
            button type="submit" { (t("kernel.usage.package.save")) }
        }
    }
}

/// The package form.
#[derive(Debug, Default, Deserialize)]
pub(super) struct PackageForm {
    #[serde(default)]
    package: String,
    #[serde(default)]
    custom: String,
    #[serde(default)]
    expected: String,
}

/// 303 to the LINE card with a one-shot flash (`ok`, catalog key).
fn redirect(s: &AppState, ok: bool, key: &str) -> Response {
    let secure = s.config.staff.cookie_secure;
    let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, BACK)]).into_response();
    resp.headers_mut().append(
        header::SET_COOKIE,
        crate::auth::flash_cookie(ok, key, secure),
    );
    resp
}

/// The choice a posted form asks for, or the catalog key of why not.
fn requested(f: &PackageForm) -> Result<Choice, &'static str> {
    match f.package.as_str() {
        "custom" => package::custom(&f.custom)
            .map(Choice::Custom)
            .ok_or("kernel.usage.package.err_custom"),
        id => package::package(id)
            .map(Choice::Package)
            .ok_or("kernel.usage.package.err_unknown"),
    }
}

pub(super) async fn submit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(f): Form<PackageForm>,
) -> Response {
    if !store::kinds(&s.deployment)
        .iter()
        .any(|k| k == package::KIND)
    {
        return redirect(&s, false, "kernel.usage.package.err_no_line");
    }
    let cur = match package::current(&s.services, &s.config).await {
        Ok(c) => c,
        Err(e) => return failed(&s, &staff, &e),
    };
    if cur.version() != f.expected {
        return redirect(&s, false, "kernel.admin.conflict");
    }
    let actor = Actor::staff(&staff);
    if f.package == CONFIG {
        let Some(row) = cur.stored.as_deref() else {
            return redirect(&s, false, "kernel.usage.package.same");
        };
        // `null` in the audit row when no Quota applies after the reset.
        let fallback = Choice::Config(cur.config).encode();
        let reset = crate::kernel::admin::reset_setting_row(
            &s.services,
            &actor,
            KEY,
            "",
            row,
            fallback.as_deref(),
            &ctx,
            &|_| Ok(Vec::new()),
        )
        .await;
        return match reset {
            Ok(true) => redirect(&s, true, "kernel.usage.package.reset_done"),
            Ok(false) => redirect(&s, false, "kernel.admin.conflict"),
            Err(e) => failed(&s, &staff, &e),
        };
    }
    let want = match requested(&f) {
        Ok(c) => c,
        Err(key) => return redirect(&s, false, key),
    };
    let value = want.encode().unwrap_or_default();
    // What is in force now as a stored value: the row, or the TOML number as
    // Custom. Choosing that again changes nothing.
    let before = cur.stored.clone().or_else(|| cur.choice.encode());
    if before.as_deref() == Some(value.as_str()) {
        return redirect(&s, false, "kernel.usage.package.same");
    }
    let saved = crate::kernel::admin::save_setting_row(
        &s.services,
        &actor,
        KEY,
        "",
        &value,
        cur.stored.as_deref(),
        before.as_deref(),
        &ctx,
        &|_| Ok(Vec::new()),
    )
    .await;
    match saved {
        Ok(true) => redirect(&s, true, "kernel.usage.package.saved"),
        Ok(false) => redirect(&s, false, "kernel.admin.conflict"),
        Err(e) => failed(&s, &staff, &e),
    }
}

/// 500 inside the layout; the error text is never shown.
fn failed(s: &AppState, staff: &CurrentStaff, _e: &crate::Error) -> Response {
    let text = s.catalog.t(&staff.locale, "kernel.auth.error");
    let body = html! { div class="card" { p { (text) } } };
    let page = crate::ui::back_office(s, staff, &text, body);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::response::Html(page.into_string()),
    )
        .into_response()
}
