//! The review step of a privacy notice version change (#135). Changing
//! the version asks linked Students to consent again, so `POST
//! /admin/texts/consent` without `reviewed` answers this page instead of
//! saving: the current and new versions, how many Students will be asked
//! again (counted from their Links and Consents), and an explicit
//! confirm, danger-styled and naming that number when it is not 0. The
//! confirm posts the same form with `reviewed=1` and the number seen in
//! `reach`; the handler counts again and shows this page once more when
//! the number moved, so nobody confirms a reach they did not read.
//!
//! The version field on `/admin/texts` lists its allowed characters and
//! checks them as Staff type ([`FIELD_JS`]); the server checks again.

use axum::http::StatusCode;
use axum::response::Response;
use maud::{Markup, PreEscaped, html};

use super::texts::th_en;
use super::view::View;
use crate::Services;
use crate::manifest::CatalogEntry as E;
use crate::port_types::{Statement, Value};

/// The posted field saying the review step was seen.
pub(super) const REVIEWED: &str = "reviewed";
/// The posted field carrying the number of Students the review showed.
pub(super) const REACH: &str = "reach";

/// How many Students changing the notice to `new` asks to consent again:
/// Students with a live Link (active or unreachable) who have not
/// accepted `new` yet, and who accepted some version before or whose
/// account already waits on the Card. The chat flow asks exactly these
/// (`kernel::flow::linked`); a seeded Link with no Consent at all is left
/// alone there, so it is not counted here.
pub(super) async fn asked_again(s: &Services, new: &str) -> crate::Result<u64> {
    let q = Statement::new(
        "SELECT count(DISTINCT l.student_id) AS n FROM link l \
         JOIN channel_account ca ON ca.id = l.channel_account_id \
         WHERE l.status <> 'revoked' \
         AND NOT EXISTS (SELECT 1 FROM consent c \
             WHERE c.student_id = l.student_id AND c.notice_version = ?) \
         AND (EXISTS (SELECT 1 FROM consent c WHERE c.student_id = l.student_id) \
             OR ca.flow_state = 'awaiting_reconsent')",
    )
    .bind(new.to_string());
    let n = s.db.query(q).await?.get(0, "n").and_then(Value::as_i64);
    Ok(n.and_then(|n| u64::try_from(n).ok()).unwrap_or(0))
}

/// What the review shows and re-posts.
pub(super) struct Change<'a> {
    pub(super) before: &'a str,
    pub(super) new: &'a str,
    /// The `expected` field of the form (the stored row's version).
    pub(super) expected: &'a str,
    pub(super) reach: u64,
    /// The number moved since the last review.
    pub(super) changed: bool,
}

/// The review page.
pub(super) fn page(v: &View<'_>, c: &Change<'_>) -> Response {
    let title = v.t("kernel.admin.consent.review_title");
    let n = crate::ui::number(c.reach);
    let fill = |k: &str| v.t(k).replace("{n}", &n);
    let body = html! {
        p { a class="back" href="/admin/texts#consent" { "← " (v.t("kernel.admin.texts.back")) } }
        h1 { (title) }
        @if c.changed {
            p class="note" role="status" { (v.t("kernel.admin.consent.changed")) }
        }
        p class="muted" { (v.t("kernel.admin.consent.review_lead")) }
        section class="card" {
            dl class="facts" {
                dt { (v.t("kernel.admin.consent.old")) }
                dd { strong class="code" { (c.before) } }
                dt { (v.t("kernel.admin.consent.new")) }
                dd { strong class="code" { (c.new) } }
                dt { (v.t("kernel.admin.consent.reach")) }
                dd data-count="reconsent" {
                    @if c.reach == 0 {
                        (v.t("kernel.admin.consent.reach_none"))
                    } @else {
                        strong { (fill("kernel.admin.consent.reach_n")) }
                    }
                }
            }
            p { (v.t("kernel.admin.consent.what_happens")) }
        }
        div class="actions" {
            form method="post" action="/admin/texts/consent" class="inline" {
                (v.staff.csrf_field())
                input type="hidden" name="version" value=(c.new);
                input type="hidden" name="expected" value=(c.expected);
                input type="hidden" name=(REVIEWED) value="1";
                input type="hidden" name=(REACH) value=(c.reach);
                @if c.reach > 0 {
                    (crate::ui::danger_button(&fill("kernel.admin.consent.confirm_n")))
                } @else {
                    button type="submit" { (v.t("kernel.admin.consent.confirm")) }
                }
            }
            a class="btn secondary" href="/admin/texts#consent" { (v.t("kernel.admin.consent.back")) }
        }
    };
    v.page(StatusCode::OK, &title, body)
}

/// The version field: its allowed characters under the label, an error
/// line the inline check fills, and that check.
pub(super) fn field(v: &View<'_>) -> Markup {
    html! {
        div class="field" {
            label for="consent-version" { (v.t("kernel.admin.texts.consent_new")) }
            p class="muted" id="consent-version-rules" { (v.t("kernel.admin.consent.rules")) }
            input id="consent-version" name="version" required maxlength="32"
                pattern="[A-Za-z0-9._\\-]+" autocomplete="off" spellcheck="false"
                aria-describedby="consent-version-rules consent-version-error";
            p class="field-error" id="consent-version-error" aria-live="polite"
                data-bad=(v.t("kernel.admin.consent.bad_chars"))
                data-space=(v.t("kernel.admin.consent.space")) {}
        }
        script { (PreEscaped(FIELD_JS)) }
    }
}

/// The inline check: every run of characters outside `A-Z a-z 0-9 . _
/// -` is quoted whole in the error line (so a Thai word keeps its vowels
/// and tone marks; a space is named by its word) and the field marked
/// invalid; a clean value clears both. No library, no build step.
const FIELD_JS: &str = r#"(function(){
var i=document.getElementById('consent-version'),e=document.getElementById('consent-version-error');
if(!i||!e)return;
i.addEventListener('input',function(){
var seen=[],space=false;function add(w){if(seen.indexOf(w)<0)seen.push(w);}
(i.value.match(/[^A-Za-z0-9._-]+/g)||[]).forEach(function(r){space=space||/\s/.test(r);
r.split(/\s+/).forEach(function(w){if(w)add('\u201c'+w+'\u201d');});});
if(space)add(e.getAttribute('data-space'));
if(seen.length){var t=e.getAttribute('data-bad').replace('{chars}',seen.join(' '));
if(e.textContent!==t)e.textContent=t;i.setAttribute('aria-invalid','true');}
else{if(e.textContent)e.textContent='';i.removeAttribute('aria-invalid');}});
})();"#;

/// The texts of the review step and the version field.
pub(super) const TEXTS: &[E] = th_en![
    // The card's button: it opens the review step, it no longer saves.
    (
        "kernel.admin.consent.review",
        "ตรวจก่อนเปลี่ยนฉบับ",
        "Review the change"
    ),
    (
        "kernel.admin.consent.rules",
        "ใช้ได้เฉพาะตัวอักษรภาษาอังกฤษ ตัวเลข จุด (.) ขีดล่าง (_) และขีดกลาง (-) ไม่เกิน 32 ตัว เช่น 2026-11",
        "Letters A-Z, digits, dot (.), underscore (_) and hyphen (-), up to 32, for example 2026-11"
    ),
    (
        "kernel.admin.consent.bad_chars",
        "ใช้ตัวนี้ไม่ได้: {chars}",
        "Not allowed: {chars}"
    ),
    ("kernel.admin.consent.space", "ช่องว่าง", "space"),
    (
        "kernel.admin.consent.review_title",
        "ตรวจก่อนเปลี่ยนฉบับประกาศความเป็นส่วนตัว",
        "Review before changing the privacy notice version"
    ),
    (
        "kernel.admin.consent.review_lead",
        "ยังไม่ได้เปลี่ยน ตรวจฉบับและจำนวนนักศึกษาที่จะถูกขอความยินยอมใหม่ แล้วกดยืนยัน",
        "Nothing has changed yet. Check the versions and how many Students will be asked again, then confirm."
    ),
    ("kernel.admin.consent.old", "ฉบับปัจจุบัน", "Current version"),
    ("kernel.admin.consent.new", "ฉบับใหม่", "New version"),
    (
        "kernel.admin.consent.reach",
        "ขอความยินยอมใหม่",
        "Asked again"
    ),
    (
        "kernel.admin.consent.reach_n",
        "นักศึกษา {n} คน",
        "{n} Students"
    ),
    (
        "kernel.admin.consent.reach_none",
        "ไม่มีนักศึกษาที่ต้องยอมรับใหม่ในตอนนี้",
        "No Student needs to accept again right now"
    ),
    (
        "kernel.admin.consent.what_happens",
        "นักศึกษาที่ผูกบัญชีแล้วจะได้รับประกาศให้ยอมรับใหม่เมื่อส่งข้อความครั้งถัดไป และจะใช้เมนูต่อได้หลังจากกดยอมรับ",
        "Linked Students get the notice to accept again with their next message, and can use the Menu again once they accept."
    ),
    (
        "kernel.admin.consent.confirm_n",
        "เปลี่ยนฉบับ: ขอความยินยอมใหม่ {n} คน",
        "Change the version: ask {n} Students again"
    ),
    (
        "kernel.admin.consent.confirm",
        "ยืนยันเปลี่ยนฉบับ",
        "Confirm the change"
    ),
    ("kernel.admin.consent.back", "กลับไปแก้ไข", "Back to editing"),
    (
        "kernel.admin.consent.changed",
        "จำนวนนักศึกษาเปลี่ยนไปจากที่ตรวจไว้ ยังไม่ได้เปลี่ยนฉบับ ตรวจอีกครั้งแล้วกดยืนยัน",
        "The number of Students changed since you checked. Nothing has changed; check again and confirm."
    ),
];
