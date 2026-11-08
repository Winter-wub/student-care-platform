//! `GET /admin`: the term status home (#123). Not a copy of the
//! navigation: one ledger of status cards, the Kernel's (the last roster
//! import, the message quota, the last backup) and each Module's
//! ([`crate::ModuleRegistrar::status_card`]; announcements add theirs),
//! sorted by order. Each card is a state, one fact and one next action.
//!
//! A card's source runs here only, and only when the Staff member holds
//! its Permission, so the home costs one or two queries per visible card
//! and no other page pays for it. A source that fails shows "could not
//! read" in its place; the rest of the home renders.

mod cards;
mod texts;

use axum::extract::State;
use axum::response::Html;
use maud::{Markup, html};

use crate::module::ModuleRegistrar;
use crate::{AppState, CardState, CurrentStaff, StatusCard};

/// The Kernel's cards, and the shell's and the home's texts.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    r.messages(texts::DEFAULTS)
        .status_card(cards::ROSTER, 10, cards::roster)
        .status_card(cards::QUOTA, 30, cards::quota)
        .status_card(cards::BACKUP, 40, cards::backup);
}

/// The cards `staff` may see, in order; `None` for a source that failed.
async fn cards(s: &AppState, staff: &CurrentStaff) -> Vec<Option<StatusCard>> {
    let mut out = Vec::new();
    for spec in s.deployment.status_cards() {
        if staff.can(spec.permission) {
            let card = spec.source.card(s.clone(), staff.clone()).await;
            out.push(card.ok());
        }
    }
    out
}

pub(crate) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Html<String> {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let title = t("kernel.home.title");
    let shown = cards(&s, &staff).await;
    let body = html! {
        h1 { (title) }
        p class="muted" {
            (t("kernel.backoffice.welcome")) " " (staff.display_name) " · " (t("kernel.home.intro"))
        }
        @if shown.is_empty() {
            div class="card" {
                p { (t(if staff.nav().is_empty() { "kernel.backoffice.no_pages" } else { "kernel.home.none" })) }
            }
        } @else {
            section class="card term" aria-label=(title) {
                ul class="status-list" {
                    @for c in &shown { (card(&t, c.as_ref())) }
                }
            }
        }
    };
    Html(crate::ui::back_office(&s, &staff, &title, body).into_string())
}

fn card(t: &dyn Fn(&str) -> String, c: Option<&StatusCard>) -> Markup {
    let Some(c) = c else {
        return html! {
            li class="status-card" data-state="warn" {
                h2 { span class="chip caution" { (t("kernel.home.state_warn")) } }
                p class="fact" { (t("kernel.home.unreadable")) }
            }
        };
    };
    let (chip, key) = match c.state {
        CardState::Empty => ("chip", "kernel.home.state_empty"),
        CardState::Ok => ("chip ok", "kernel.home.state_ok"),
        CardState::Warn => ("chip caution", "kernel.home.state_warn"),
    };
    html! {
        li class="status-card" data-state=(c.state.as_str()) {
            h2 { (c.title) " " span class=(chip) { (t(key)) } }
            p class="fact" { (c.fact) }
            @if let Some((label, href)) = &c.action {
                a class="btn secondary" href=(href) { (label) }
            }
        }
    }
}
