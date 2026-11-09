//! `GET /admin`: the term status home (#123). Not a copy of the
//! navigation: one ledger of status cards, the Kernel's (the last roster
//! import, the message quota, the last backup) and each Module's
//! ([`crate::ModuleRegistrar::status_card`]; announcements add theirs),
//! sorted by order. Each card is a title, a state, one fact and one next
//! action.
//!
//! A card's source runs here only, and only when the Staff member holds
//! its Permission, so the home costs one or two queries per visible card
//! and no other page pays for it. The sources run one after another: the
//! workspace has no `futures` crate for a join, and no new crate is added
//! for it. A source that fails shows its title with "could not read"; the
//! rest of the home renders.

mod cards;
mod checklist;
mod texts;

use axum::extract::State;
use axum::response::Html;
use maud::{Markup, html};

use crate::module::ModuleRegistrar;
use crate::{AppState, CardState, CurrentStaff, StatusCard};

/// The Kernel's cards, and the shell's and the home's texts.
pub(crate) fn register(r: &mut ModuleRegistrar) {
    r.messages(texts::DEFAULTS)
        .messages(crate::ui::ONBOARD_TEXTS)
        .status_card(cards::ROSTER, "kernel.home.roster.title", 10, cards::roster)
        .status_card(cards::QUOTA, "kernel.home.quota.title", 30, cards::quota)
        .status_card(cards::BACKUP, "kernel.home.backup.title", 40, cards::backup);
}

/// One card as shown: its title, and the card or `None` when its source
/// failed.
type Shown = (String, Option<StatusCard>);

/// The cards `staff` may see, in order.
async fn cards(s: &AppState, staff: &CurrentStaff) -> Vec<Shown> {
    let mut out = Vec::new();
    for spec in s.deployment.status_cards() {
        if staff.can(spec.permission) {
            let title = s.catalog.t(&staff.locale, spec.title_key);
            let card = spec.source.card(s.clone(), staff.clone()).await;
            out.push((title, card.ok()));
        }
    }
    out
}

pub(crate) async fn page(State(s): State<AppState>, staff: CurrentStaff) -> Html<String> {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let title = t("kernel.home.title");
    let shown = cards(&s, &staff).await;
    let setup = checklist::block(&s, &staff).await;
    let body = html! {
        h1 { (title) }
        p class="muted" {
            (t("kernel.backoffice.welcome")) " " (staff.display_name) " · " (t("kernel.home.intro"))
        }
        (setup)
        @if shown.is_empty() {
            div class="card" {
                p { (t(if staff.nav().is_empty() { "kernel.backoffice.no_pages" } else { "kernel.home.none" })) }
            }
        } @else {
            section class="card term" aria-label=(title) {
                ul class="status-list" {
                    @for (name, c) in &shown { (card(&t, name, c.as_ref())) }
                }
            }
        }
    };
    Html(crate::ui::back_office(&s, &staff, &title, body).into_string())
}

fn card(t: &dyn Fn(&str) -> String, title: &str, c: Option<&StatusCard>) -> Markup {
    let state = c.map_or(CardState::Warn, |c| c.state);
    let (chip, key) = match state {
        CardState::Empty => ("chip", "kernel.home.state_empty"),
        CardState::Ok => ("chip ok", "kernel.home.state_ok"),
        CardState::Warn => ("chip caution", "kernel.home.state_warn"),
    };
    html! {
        li class="status-card" data-state=(state.as_str()) data-failed[c.is_none()] {
            h2 { (title) " " span class=(chip) { (t(key)) } }
            @match c {
                Some(c) => {
                    p class="fact" { (c.fact) }
                    @if let Some((label, href)) = &c.action {
                        a class="btn secondary" href=(href) { (label) }
                    }
                }
                None => p class="fact" { (t("kernel.home.unreadable")) },
            }
        }
    }
}
