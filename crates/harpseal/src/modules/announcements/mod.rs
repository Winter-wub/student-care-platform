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
//!   are `skipped`. The Audience can name Students by hand (their student
//!   ids, pasted or picked): ANDed with the other filters. "ส่งซ้ำ" is a new round with a new Job key.
//! - **Chat**: the Card's "รับทราบ" button is Action `announcements.ack.<id>`
//!   ([`ack`]); Menu entry `announcements.latest` ("ข่าวล่าสุด", [`latest`]): the
//!   `[announcements] latest` newest (default 5, `1..=10`).
//! - **Edit / retract**: Jobs `announcements.edit` (`EditMessage` where
//!   the Channel has it) and `announcements.retract` (`DeleteMessage`,
//!   within 48 hours, plus a late pass 16 minutes later), each fanned out
//!   into paced chunk Jobs that resume after a rate limit. An edit saved
//!   with "notify recipients of the edit" also runs `announcements.notice`
//!   (Module migration `0002_edit_notice`): one metered Push per recipient
//!   whose text changed on Channels that cannot edit (LINE), linking to the
//!   updated announcement; Students who already acknowledged get it too
//!   and their acknowledgement stands. Each edit notifies again.
//! - **Expiry**: past `expires_at` an announcement leaves the Menu and the
//!   Mini App list, and a Student opening or acknowledging it (chat Action,
//!   Mini App deep link) is told it is no longer available (#77). Staff
//!   still see it, marked expired.
//! - **Files** (#109, Module migration `0003_files`): up to 4 JPEG or PNG
//!   images per announcement in `announcement_file` and the FileStore;
//!   served publicly at `/announcements/files/<token>` only while it is
//!   sent and live; the first is the chat
//!   Card's image, and the Mini App shows each in place. Up to 3 PDFs
//!   (#113) of 10 MB each the same way: the Mini App lists them with
//!   their names and sizes, and the first is a URL button on the Card.
//! - **Saved Audiences** (#144, Module migration `0004_saved_audiences`):
//!   Staff name the editor's Audience ("บันทึกกลุ่มผู้รับนี้") and pick it
//!   again later; it keeps the rules (the choice and the filters), never a
//!   list, so the reach is counted again on the editor and the review
//!   step. A value `[roster]` no longer has is shown and refused, never
//!   dropped. Rename and delete at `/admin/announcements/audiences`; every
//!   write is audited and needs `announcements.send`.
//! - **Pages** under `/admin/announcements`; Permissions
//!   `announcements.view`, `announcements.send`, `announcements.retract`.
//! - **Confirmation in proportion to reach** (#120): the Audience is an
//!   explicit choice (`scope`: ทุกคน / เลือกกลุ่ม / เลือกรายคน; empty filters
//!   never mean everyone), and "ตรวจก่อนส่ง" (#157) opens a review step
//!   (`page_review`). What guards what:
//!   - the review step and its `reviewed` field only guard against an
//!     accidental click, for group and hand-picked sends: a POST can carry
//!     `reviewed=1` without the page having been seen;
//!   - CSRF and the Permission `announcements.send` are the security gates;
//!   - a send or resend to everyone is a server gate: the POST must carry
//!     `reach`, and it must equal a recount of the Audience at that moment
//!     (else nothing is sent);
//!   - a scheduled announcement whose Audience changes is held as a draft
//!     and goes out again only through the review step (`page_review::hold`;
//!     `service::update` refuses the change on a schedule);
//!   - the version compare-and-set keeps a double submit to one send.
//! - **From code**: [`create_draft`] and [`confirm`] take the same path as
//!   the pages (Permission `announcements.send` checked, audited as the
//!   acting Staff member); [`create_draft`] takes images and PDFs
//!   ([`AnnouncementFile`]) with the form's checks (#164).

mod api;
mod api_files;
mod chat;
mod deliver;
mod draft;
mod files;
mod files_pdf;
mod files_pdf_scan;
mod files_serve;
mod files_sniff;
mod files_stage;
mod form;
mod form_attrs;
mod form_images;
mod form_pdfs;
mod form_scope;
mod gate;
mod maint;
mod maint_chunk;
mod notice;
mod outcome;
mod page_actions;
mod page_audience;
mod page_count;
mod page_detail;
mod page_editor;
mod page_form;
mod page_list;
mod page_more;
mod page_notice;
mod page_pick;
mod page_review;
mod page_saved;
mod page_saved_list;
mod saved;
mod service;
mod stats;
mod status;
mod store;
mod student;
mod sweep;
mod texts;
mod texts_audience;
mod texts_form;
mod texts_review;
mod texts_saved;
mod view;

use axum::routing::{get, post};

pub use api::{
    AnnouncementFile, FileKind, NewAnnouncement, SEND_PERMISSION, confirm, create_draft,
    create_draft_owned,
};
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
pub(crate) const EDIT_CHUNK: &str = "announcements.edit.chunk";
pub(crate) const RETRACT_CHUNK: &str = "announcements.retract.chunk";
pub(crate) const NOTICE: &str = "announcements.notice";
pub(crate) const NOTICE_CHUNK: &str = "announcements.notice.chunk";
pub(crate) const SWEEP: &str = "announcements.sweep";
/// How often stuck rounds are looked for.
const SWEEP_SCHEDULE: &str = "*/10 * * * *";

/// Back-office navigation order (after Roster).
const NAV_ORDER: u16 = 30;

const PERMISSIONS: &[Permission] = &[
    Permission::new(
        "announcements.view",
        "See announcements and who acknowledged",
    )
    .thai("ดูประกาศและรายชื่อผู้รับทราบ"),
    Permission::new(
        "announcements.send",
        "Create, edit, send and resend announcements",
    )
    .thai("เขียน แก้ไข และส่งประกาศ"),
    Permission::new("announcements.retract", "Withdraw announcements").thai("ถอนประกาศ"),
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
        .migration(
            "0002_edit_notice",
            include_str!("../../../migrations/announcements/0002_edit_notice.sql"),
        )
        .migration(
            "0003_files",
            include_str!("../../../migrations/announcements/0003_files.sql"),
        )
        .migration(
            "0004_saved_audiences",
            include_str!("../../../migrations/announcements/0004_saved_audiences.sql"),
        )
        .messages(texts::CHAT)
        .messages(texts::PAGES)
        .messages(texts_form::FORM)
        .messages(texts_review::REVIEW)
        .messages(texts_saved::SAVED)
        .messages(texts_audience::AUDIENCE)
        .messages(page_editor::EDITOR)
        .messages(form_images::TEXTS)
        .messages(form_pdfs::TEXTS)
        .menu_entry(latest, chat::on_latest)
        .action_prefix(ACK_PREFIX, chat::on_ack)
        .mini_app_button(MINI_APP_PATH)
        .job(JobSpec::new(DELIVER), deliver::parent)
        .job(JobSpec::new(DELIVER_CHANNEL), deliver::child)
        .job(JobSpec::new(EDIT), maint::edit)
        .job(JobSpec::new(RETRACT), maint::retract)
        .job(JobSpec::new(EDIT_CHUNK), maint_chunk::edit_chunk)
        .job(JobSpec::new(RETRACT_CHUNK), maint_chunk::retract_chunk)
        .job(JobSpec::new(NOTICE), notice::notice)
        .job(JobSpec::new(NOTICE_CHUNK), notice::notice_chunk)
        .job(JobSpec::scheduled(SWEEP, SWEEP_SCHEDULE), sweep::job)
        .messages(status::TEXTS)
        .status_card(
            status::PERMISSION,
            status::TITLE,
            status::ORDER,
            status::card,
        );
        pages(r);
    }
}

/// The create / edit form takes images (#109) and PDFs (#113): its body
/// may hold [`files::MAX_IMAGES`] of [`files::MAX_IMAGE_BYTES`] and
/// [`files::MAX_PDFS`] of [`files::MAX_PDF_BYTES`]; a larger request is
/// refused before it is read.
fn upload(path: &'static str) -> crate::module::UploadSpec {
    crate::module::UploadSpec {
        path,
        max_body: files::FORM_BODY_LIMIT,
        too_large: None,
        columns: None,
    }
}

fn pages(r: &mut ModuleRegistrar) {
    r.public("/announcements/files/{token}", get(files_serve::public));
    let (view, send) = ("announcements.view", "announcements.send");
    r.back_office_page(
        view,
        "/announcements",
        "announcements.nav",
        NAV_ORDER,
        get(page_list::page),
    )
    .back_office_upload(
        send,
        upload("/announcements/new"),
        get(page_form::new_page).post(page_form::new_post),
    )
    .back_office(send, "/announcements/audience", post(page_count::fragment))
    .back_office(
        send,
        "/announcements/preview",
        post(page_editor::fragment).layer(axum::extract::DefaultBodyLimit::max(
            page_editor::PREVIEW_MAX_BYTES,
        )),
    )
    .back_office(
        send,
        "/announcements/pick",
        get(page_pick::results).post(page_pick::add),
    )
    .back_office(
        send,
        "/announcements/audiences",
        get(page_saved_list::page).post(page_saved::save),
    )
    .back_office(
        send,
        "/announcements/audiences/picker",
        get(page_saved::picker),
    )
    .back_office(
        send,
        "/announcements/audiences/use",
        post(page_saved::use_saved),
    )
    .back_office(
        send,
        "/announcements/audiences/{id}/rename",
        post(page_saved_list::rename),
    )
    .back_office(
        send,
        "/announcements/audiences/{id}/delete",
        get(page_saved_list::delete_page).post(page_saved_list::delete),
    )
    .back_office(view, "/announcements/{id}", get(page_detail::page))
    .back_office_upload(
        send,
        upload("/announcements/{id}/edit"),
        get(page_form::edit_page).post(page_form::edit_post),
    )
    .back_office(
        view,
        "/announcements/{id}/files/{token}",
        get(files_serve::staff),
    )
    .back_office(send, "/announcements/{id}/review", get(page_review::page))
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
