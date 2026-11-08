//! The body editor of the create / edit form (#108): a toolbar that
//! inserts the format's syntax ([`crate::rich_text`]), a help line, and a
//! preview of the body as the Mini App shows it (the same
//! [`crate::rich_text::html`]), refreshed by `POST
//! /admin/announcements/preview` (an HTMX fragment). Without script the
//! toolbar stays hidden, the help line shows the syntax and the preview
//! is the one rendered with the page.

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, PreEscaped, html};

use super::form::{Fields, MAX_BODY};
use super::view::View;
use crate::manifest::CatalogEntry as E;
use crate::rich_text;
use crate::{AppState, CurrentStaff};

/// The preview route (Permission `announcements.send`).
pub(super) const PATH: &str = "/admin/announcements/preview";
/// Largest preview request: the whole form (two bodies of 2,000 Thai
/// characters percent-encoded are about 36 KB, a hand-picked list of
/// 2,000 ids about 20 KB), with room to spare.
pub(super) const PREVIEW_MAX_BYTES: usize = 128 * 1024;

/// Editor texts, Thai and English.
pub(crate) const EDITOR: &[E] = &[
    E::new("announcements.editor.toolbar", "th", "จัดรูปแบบ"),
    E::new("announcements.editor.toolbar", "en", "Formatting"),
    E::new("announcements.editor.bold", "th", "ตัวหนา"),
    E::new("announcements.editor.bold", "en", "Bold"),
    E::new("announcements.editor.italic", "th", "ตัวเอียง"),
    E::new("announcements.editor.italic", "en", "Italic"),
    E::new("announcements.editor.list", "th", "รายการ"),
    E::new("announcements.editor.list", "en", "Bulleted list"),
    E::new("announcements.editor.numbered", "th", "รายการแบบมีเลข"),
    E::new("announcements.editor.numbered", "en", "Numbered list"),
    E::new("announcements.editor.link", "th", "ลิงก์"),
    E::new("announcements.editor.link", "en", "Link"),
    E::new("announcements.editor.placeholder", "th", "ข้อความ"),
    E::new("announcements.editor.placeholder", "en", "text"),
    E::new(
        "announcements.editor.help",
        "th",
        "จัดรูปแบบได้: **ตัวหนา** *ตัวเอียง* บรรทัดที่ขึ้นต้นด้วย \"- \" หรือ \"1. \" เป็นรายการ \
         [ข้อความ](https://…) เป็นลิงก์ ลิงก์ต้องขึ้นต้นด้วย https:// เท่านั้น \
         ลิงก์สองอันแรกจะเป็นปุ่มในแชทด้วย",
    ),
    E::new(
        "announcements.editor.help",
        "en",
        "Formatting: **bold** *italic*, lines starting \"- \" or \"1. \" are lists, \
         [text](https://…) is a link. Links must start with https://. \
         The first two links also become buttons in chat.",
    ),
    E::new(
        "announcements.editor.too_long",
        "th",
        "ยาวเกิน 2,000 ตัวอักษร จึงแสดงตัวอย่างไม่ได้",
    ),
    E::new(
        "announcements.editor.too_long",
        "en",
        "Over 2,000 characters: no preview.",
    ),
    E::new("announcements.editor.preview", "th", "ตัวอย่างในมินิแอป"),
    E::new("announcements.editor.preview", "en", "In the Mini App"),
];

/// Inserts the syntax around the selection (lists: before each selected
/// line), then fires `input` so the preview refreshes.
const TOOLBAR_JS: &str = r#"(function () {
  var done = function (ta) { ta.focus(); ta.dispatchEvent(new Event('input', { bubbles: true })); };
  var wrap = function (ta, before, after, ph) {
    var s = ta.selectionStart, e = ta.selectionEnd, v = ta.value;
    var sel = v.slice(s, e) || ph;
    ta.value = v.slice(0, s) + before + sel + after + v.slice(e);
    ta.selectionStart = s + before.length;
    ta.selectionEnd = s + before.length + sel.length;
  };
  var lines = function (ta, numbered) {
    var s = ta.selectionStart, e = ta.selectionEnd, v = ta.value;
    var start = v.lastIndexOf('\n', s - 1) + 1;
    var n = 0;
    var out = v.slice(start, e).split('\n').map(function (l) {
      n += 1; return (numbered ? n + '. ' : '- ') + l;
    }).join('\n');
    ta.value = v.slice(0, start) + out + v.slice(e);
    ta.selectionStart = start; ta.selectionEnd = start + out.length;
  };
  document.querySelectorAll('.md-toolbar').forEach(function (bar) {
    var ta = document.getElementById(bar.dataset.for);
    if (!ta) { return; }
    bar.hidden = false;
    bar.addEventListener('click', function (ev) {
      var b = ev.target.closest('button[data-md]');
      if (!b) { return; }
      var k = b.dataset.md, ph = bar.dataset.ph;
      if (k === 'bold') { wrap(ta, '**', '**', ph); }
      else if (k === 'italic') { wrap(ta, '*', '*', ph); }
      else if (k === 'list') { lines(ta, false); }
      else if (k === 'numbered') { lines(ta, true); }
      else if (k === 'link') {
        wrap(ta, '[', '](https://)', ph);
        var p = ta.selectionEnd + 2;
        ta.selectionStart = p; ta.selectionEnd = p + 8;
      }
      done(ta);
    });
  });
})();"#;

/// The toolbar of textarea `id` (hidden until the script runs) and the
/// help line.
pub(super) fn toolbar(v: &View<'_>, id: &str) -> Markup {
    let buttons = [
        ("bold", "B", "announcements.editor.bold"),
        ("italic", "I", "announcements.editor.italic"),
        ("list", "•", "announcements.editor.list"),
        ("numbered", "1.", "announcements.editor.numbered"),
        ("link", "🔗", "announcements.editor.link"),
    ];
    html! {
        div class="md-toolbar" role="toolbar" hidden data-for=(id)
            data-ph=(v.t("announcements.editor.placeholder"))
            aria-label=(v.t("announcements.editor.toolbar")) aria-controls=(id) {
            @for (kind, face, key) in buttons {
                button type="button" class="secondary" data-md=(kind)
                    title=(v.t(key)) aria-label=(v.t(key)) { (face) }
            }
        }
    }
}

/// The help line under a body.
pub(super) fn help(v: &View<'_>) -> Markup {
    html! { p class="muted" { (v.t("announcements.editor.help")) } }
}

/// The toolbar script, once per page.
pub(super) fn script() -> Markup {
    html! { script { (PreEscaped(TOOLBAR_JS)) } }
}

/// The Mini App preview of the bodies in `f` (Thai, then English when
/// written), refreshed on every change of the form.
pub(super) fn preview(v: &View<'_>, f: &Fields) -> Markup {
    shell(html! {
        p class="muted" { (v.t("announcements.editor.preview")) }
        div class="mini-preview" lang="th" { (rich_text::html(&f.body_th)) }
        @if !f.body_en.trim().is_empty() {
            div class="mini-preview" lang="en" { (rich_text::html(&f.body_en)) }
        }
    })
}

/// The preview box around `inner`, refreshed on every change of the form.
fn shell(inner: Markup) -> Markup {
    html! {
        div id="body-preview" class="card preview" hx-post=(PATH)
            hx-trigger="input changed delay:400ms from:closest form" hx-include="closest form"
            hx-target="this" hx-swap="outerHTML" aria-live="polite" { (inner) }
    }
}

/// `POST /admin/announcements/preview`: [`preview`] of the posted form,
/// or, for a body over [`MAX_BODY`] characters (the form's limit), the
/// line "ยาวเกิน 2,000 ตัวอักษร" without parsing it. A request over
/// [`PREVIEW_MAX_BYTES`] gets a 413 from the route's `DefaultBodyLimit`.
pub(super) async fn fragment(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let f = Fields::from_pairs(&pairs);
    let v = View::new(&s, &staff);
    // The form's own limit, checked before anything is parsed. A 200 with
    // a line, not a 413: HTMX swaps no error status, so Staff would see
    // the old preview and nothing else.
    let long = |b: &str| b.chars().count() > MAX_BODY;
    if long(&f.body_th) || long(&f.body_en) {
        let line = shell(html! {
            p class="field-error" role="alert" { (v.t("announcements.editor.too_long")) }
        });
        return (StatusCode::OK, Html(line.into_string())).into_response();
    }
    (StatusCode::OK, Html(preview(&v, &f).into_string())).into_response()
}
