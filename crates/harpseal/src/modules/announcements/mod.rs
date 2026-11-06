//! The Announcements Module (#10, #25): Staff send an announcement to an
//! Audience and see who acknowledged; Students receive it in chat as a
//! Card and acknowledge it.
//!
//! Add it with `.module(Announcements)`. It declares a Mini App button
//! (`/app/announcements/<id>`), so `build()` requires every Channel to
//! have a Mini App.
//!
//! - **Tables** (Module migration `0001_announcements`): `announcement`,
//!   `announcement_text` (Thai required, a Student gets their Locale with
//!   Thai fallback), `announcement_delivery` (one row per round per live
//!   Link of the Audience).
//! - **Sending**: confirming a draft enqueues `announcements.deliver` (now,
//!   or at `send_at`); it snapshots the [`crate::audience`] into delivery
//!   rows and enqueues one `announcements.deliver.channel` Job per Channel
//!   per multicast group (LINE: 500) or per 25 accounts (paced), which send
//!   through [`crate::Deployment::send_many_with_key`]. Unreachable Links
//!   are `skipped`. "ส่งซ้ำ" is a new round with a new Job key.
//! - **Chat**: the Card's "รับทราบ" button is Action `announcements.ack.<id>`
//!   ([`ack`]); Menu entry `announcements.latest` ("ข่าวล่าสุด", [`latest`]).
//! - **Edit / retract**: Jobs `announcements.edit` (`EditMessage` where
//!   the Channel has it) and `announcements.retract` (`DeleteMessage`,
//!   within 48 hours).
//! - **Pages** under `/admin/announcements`; Permissions
//!   `announcements.view`, `announcements.send`, `announcements.retract`.

mod chat;
mod deliver;
mod form;
mod gate;
mod maint;
mod outcome;
mod page_actions;
mod page_detail;
mod page_form;
mod page_list;
mod service;
mod stats;
mod store;
mod student;
mod sweep;
mod texts;
mod view;

use axum::routing::{get, post};

pub use chat::{AckOutcome, ack, ack_action, detail_path, latest};
pub use stats::{RoundStats, Stats, stats};
pub use store::Summary;
pub use student::{StudentAnnouncement, detail_for_student, list_for_student};

use crate::manifest::{JobSpec, MenuEntrySpec, Permission};
use crate::message::ActionId;
use crate::{Module, ModuleRegistrar};

/// Module name: prefix of its Permissions, texts, Jobs and migrations.
pub const NAME: &str = "announcements";
/// Prefix of the ack Actions: `announcements.ack.<announcement id>`.
pub const ACK_PREFIX: &str = "announcements.ack.";
/// The "ข่าวล่าสุด" Menu entry's Action.
pub const LATEST: &str = "announcements.latest";
/// The Mini App page the Cards open (the list; `/<id>` for one).
pub const MINI_APP_PATH: &str = "/app/announcements";

pub(crate) const DELIVER: &str = "announcements.deliver";
pub(crate) const DELIVER_CHANNEL: &str = "announcements.deliver.channel";
pub(crate) const EDIT: &str = "announcements.edit";
pub(crate) const RETRACT: &str = "announcements.retract";
pub(crate) const SWEEP: &str = "announcements.sweep";
/// How often stuck rounds are looked for.
const SWEEP_SCHEDULE: &str = "*/10 * * * *";

/// Back-office navigation order (after Roster).
const NAV_ORDER: u16 = 30;

const PERMISSIONS: &[Permission] = &[
    Permission::new(
        "announcements.view",
        "See announcements and who acknowledged",
    ),
    Permission::new(
        "announcements.send",
        "Create, edit, send and resend announcements",
    ),
    Permission::new("announcements.retract", "Withdraw announcements"),
];

/// The built-in Announcements Module.
#[derive(Debug, Clone, Copy, Default)]
pub struct Announcements;

impl Module for Announcements {
    fn name(&self) -> &'static str {
        NAME
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        for p in PERMISSIONS {
            r.permission(*p);
        }
        let latest = MenuEntrySpec {
            label_key: "announcements.menu.latest",
            action: ActionId::new(LATEST).expect("Action id is ≤ 64 bytes"),
            order: 10,
        };
        r.migration(
            "0001_announcements",
            include_str!("../../../migrations/announcements/0001_announcements.sql"),
        )
        .messages(texts::CHAT)
        .messages(texts::PAGES)
        .messages(texts::FORM)
        .menu_entry(latest, chat::on_latest)
        .action_prefix(ACK_PREFIX, chat::on_ack)
        .mini_app_button(MINI_APP_PATH)
        .job(JobSpec::new(DELIVER), deliver::parent)
        .job(JobSpec::new(DELIVER_CHANNEL), deliver::child)
        .job(JobSpec::new(EDIT), maint::edit)
        .job(JobSpec::new(RETRACT), maint::retract)
        .job(JobSpec::scheduled(SWEEP, SWEEP_SCHEDULE), sweep::job);
        pages(r);
    }
}

fn pages(r: &mut ModuleRegistrar) {
    let (view, send) = ("announcements.view", "announcements.send");
    r.back_office_page(
        view,
        "/announcements",
        "announcements.nav",
        NAV_ORDER,
        get(page_list::page),
    )
    .back_office(
        send,
        "/announcements/new",
        get(page_form::new_page).post(page_form::new_post),
    )
    .back_office(
        send,
        "/announcements/audience",
        post(page_form::audience_fragment),
    )
    .back_office(view, "/announcements/{id}", get(page_detail::page))
    .back_office(
        send,
        "/announcements/{id}/edit",
        get(page_form::edit_page).post(page_form::edit_post),
    )
    .back_office(
        send,
        "/announcements/{id}/confirm",
        post(page_actions::confirm),
    )
    .back_office(
        send,
        "/announcements/{id}/cancel",
        post(page_actions::cancel),
    )
    .back_office(
        send,
        "/announcements/{id}/resend",
        post(page_actions::resend),
    )
    .back_office(
        "announcements.retract",
        "/announcements/{id}/retract",
        post(page_actions::retract),
    );
}
