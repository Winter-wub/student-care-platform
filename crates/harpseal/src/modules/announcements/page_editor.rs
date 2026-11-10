//! The body editor of the create / edit form (#108): a toolbar that
//! inserts the format's syntax ([`crate::rich_text`]), a help line, and a
//! preview of the body as the Mini App shows it (the same
//! [`crate::rich_text::html`]), refreshed by `POST
//! /admin/announcements/preview` (an HTMX fragment). Without script the
//! toolbar stays hidden, the help line shows the syntax and the preview
//! is the one rendered with the page.
//!
//! The preview box is not a live region (#156): it is replaced as Staff
//! type. One short line goes to `#preview-said` only when what the
//! preview shows changes (empty, the text, too long), so a screen reader
//! hears one line per change, not the whole preview every 400 ms.

use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, PreEscaped, html};

use super::files::{self, Stored};
use super::form::{Fields, MAX_BODY};
use super::view::View;
use crate::manifest::CatalogEntry as E;
use crate::rich_text::Colour;
use crate::{AppState, CurrentStaff};

/// The preview route (Permission `announcements.send`).
pub(super) const PATH: &str = "/admin/announcements/preview";
/// Largest preview request: the whole form (two bodies of 2,000 Thai
/// characters percent-encoded are about 36 KB, a hand-picked list of
/// 2,000 ids about 20 KB), with room to spare.
pub(super) const PREVIEW_MAX_BYTES: usize = 128 * 1024;

/// Editor texts, Thai and English.
pub(crate) const EDITOR: &[E] = &[
    // #127: what the empty preview says before anything is typed.
    E::new(
        "announcements.editor.preview_empty",
        "th",
        "ข้อความที่พิมพ์จะแสดงที่นี่ ตามที่นักศึกษาเห็นในมินิแอป",
    ),
    E::new(
        "announcements.editor.preview_empty",
        "en",
        "What you type appears here as Students see it in the Mini App.",
    ),
    E::new(
        "announcements.editor.card_placeholder",
        "th",
        "หัวข้อและข้อความจะแสดงในการ์ดนี้ ตามที่นักศึกษาเห็นในแชท",
    ),
    E::new(
        "announcements.editor.card_placeholder",
        "en",
        "The title and text appear in this card as Students see it in the chat.",
    ),
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
    E::new(
        "announcements.editor.heading",
        "th",
        "หัวข้อ (กดซ้ำเพื่อเปลี่ยนขนาด)",
    ),
    E::new(
        "announcements.editor.heading",
        "en",
        "Heading (press again to change its size)",
    ),
    E::new("announcements.editor.colour.red", "th", "สีแดง"),
    E::new("announcements.editor.colour.red", "en", "Red"),
    E::new("announcements.editor.colour.orange", "th", "สีส้ม"),
    E::new("announcements.editor.colour.orange", "en", "Orange"),
    E::new("announcements.editor.colour.green", "th", "สีเขียว"),
    E::new("announcements.editor.colour.green", "en", "Green"),
    E::new("announcements.editor.colour.blue", "th", "สีน้ำเงิน"),
    E::new("announcements.editor.colour.blue", "en", "Blue"),
    E::new("announcements.editor.colour.grey", "th", "สีเทา"),
    E::new("announcements.editor.colour.grey", "en", "Grey"),
    E::new("announcements.editor.placeholder", "th", "ข้อความ"),
    E::new("announcements.editor.placeholder", "en", "text"),
    E::new(
        "announcements.editor.help",
        "th",
        "จัดรูปแบบได้: **ตัวหนา** *ตัวเอียง* บรรทัดที่ขึ้นต้นด้วย \"- \" หรือ \"1. \" เป็นรายการ \
         บรรทัดที่ขึ้นต้นด้วย \"## \" หรือ \"### \" เป็นหัวข้อ {red}ข้อความ{/red} เป็นสี \
         (red, orange, green, blue, grey) [ข้อความ](https://…) เป็นลิงก์ \
         ลิงก์ต้องขึ้นต้นด้วย https:// เท่านั้น ลิงก์สองอันแรกจะเป็นปุ่มในแชทด้วย \
         ในแชทหัวข้อจะเป็นบรรทัดธรรมดาและไม่มีสี",
    ),
    E::new(
        "announcements.editor.help",
        "en",
        "Formatting: **bold** *italic*, lines starting \"- \" or \"1. \" are lists, \
         lines starting \"## \" or \"### \" are headings, {red}text{/red} is a colour \
         (red, orange, green, blue, grey), [text](https://…) is a link. \
         Links must start with https://. The first two links also become buttons in chat. \
         Chat shows headings as plain lines and no colours.",
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
    // #140: the names of the form's disclosures.
    E::new("announcements.editor.help_summary", "th", "วิธีจัดรูปแบบข้อความ"),
    E::new(
        "announcements.editor.help_summary",
        "en",
        "How to format the text",
    ),
    E::new(
        "announcements.editor.images_summary",
        "th",
        "รูปภาพ (ไม่บังคับ)",
    ),
    E::new(
        "announcements.editor.images_summary",
        "en",
        "Images (optional)",
    ),
    E::new(
        "announcements.editor.pdfs_summary",
        "th",
        "ไฟล์ PDF แนบ (ไม่บังคับ)",
    ),
    E::new(
        "announcements.editor.pdfs_summary",
        "en",
        "PDF attachments (optional)",
    ),
    E::new(
        "announcements.editor.more_summary",
        "th",
        "วันหมดอายุและการขอให้กดรับทราบ (ไม่บังคับ)",
    ),
    E::new(
        "announcements.editor.more_summary",
        "en",
        "Expiry and acknowledgement (optional)",
    ),
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
  // `## ` → `### ` → none → `## ` on the caret's line.
  var heading = function (ta) {
    var s = ta.selectionStart, v = ta.value;
    var start = v.lastIndexOf('\n', s - 1) + 1;
    var rest = v.slice(start), next;
    if (rest.indexOf('### ') === 0) { next = rest.slice(4); }
    else if (rest.indexOf('## ') === 0) { next = '#' + rest; }
    else { next = '## ' + rest; }
    ta.value = v.slice(0, start) + next;
    var p = Math.max(start, s + next.length - rest.length);
    ta.selectionStart = p; ta.selectionEnd = p;
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
      else if (k === 'heading') { heading(ta); }
      else if (k.indexOf('colour-') === 0) {
        var c = k.slice(7);
        wrap(ta, '{' + c + '}', '{/' + c + '}', ph);
      }
      else if (k === 'image') {
        // The first slot with an image (saved or chosen) the body does not
        // place yet; else slot 1.
        var n = 1;
        for (var i = 1; i <= 4; i += 1) {
          var slot = document.querySelector('[data-image-slot="' + i + '"]');
          var file = document.getElementById('image' + i);
          var has = (slot && slot.hasAttribute('data-filled')) || (file && file.files.length);
          if (has && ta.value.indexOf('(image:' + i + ')') < 0) { n = i; break; }
        }
        wrap(ta, '![', '](image:' + n + ')', ph);
      }
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
        ("heading", "H", "announcements.editor.heading"),
        ("link", "🔗", "announcements.editor.link"),
        ("image", "🖼", "announcements.editor.image"),
    ];
    html! {
        div class="md-toolbar" role="toolbar" hidden data-for=(id)
            data-ph=(v.t("announcements.editor.placeholder"))
            aria-label=(v.t("announcements.editor.toolbar")) aria-controls=(id) {
            @for (kind, face, key) in buttons {
                button type="button" class="secondary" data-md=(kind)
                    title=(v.t(key)) aria-label=(v.t(key)) { (face) }
            }
            // The palette (#113): an "A" in each colour, never a free one.
            @for c in Colour::ALL {
                @let label = v.t(&format!("announcements.editor.colour.{}", c.name()));
                button type="button" class="secondary swatch"
                    data-md={ "colour-" (c.name()) } title=(label) aria-label=(label) {
                    span class=(c.class()) { "A" }
                }
            }
        }
    }
}

/// The syntax help under a body, folded (#140): the toolbar does the same
/// without it, so it opens only when asked.
pub(super) fn help(v: &View<'_>) -> Markup {
    html! {
        details class="syntax-help" {
            summary { (v.t("announcements.editor.help_summary")) }
            p class="muted" { (v.t("announcements.editor.help")) }
        }
    }
}

/// The toolbar script, once per page.
pub(super) fn script() -> Markup {
    html! { script { (PreEscaped(TOOLBAR_JS)) } }
}

/// The Mini App preview of the bodies in `f` (Thai, then English when
/// written) with the saved `images` (#109; a file chosen but not saved
/// shows after the save), refreshed on every change of the form.
pub(super) fn preview(v: &View<'_>, f: &Fields, images: &[Stored]) -> Markup {
    let label = v.t("announcements.images.slot");
    shell(
        state_of(f),
        html! {
            p class="muted" { (v.t("announcements.editor.preview")) }
            // Nothing typed yet (#127): say what will appear, not an empty card.
            @if f.body_th.trim().is_empty() && f.body_en.trim().is_empty() {
                p class="preview-placeholder" { (v.t("announcements.editor.preview_empty")) }
            } @else {
                div class="mini-preview" lang="th" { (files::body_html(&f.body_th, images, true, &label)) }
            }
            @if !f.body_en.trim().is_empty() {
                div class="mini-preview" lang="en" { (files::body_html(&f.body_en, images, true, &label)) }
            }
        },
    )
}

/// The preview box around `inner`, refreshed on every change of the form;
/// `state` (`empty`, `shown` or `long`) goes back as `preview_was`.
fn shell(state: &str, inner: Markup) -> Markup {
    let vals = serde_json::json!({ "preview_was": state }).to_string();
    html! {
        div id="body-preview" class="card preview" hx-post=(PATH)
            hx-trigger="input changed delay:400ms from:closest form" hx-include="closest form" hx-params=(super::form_images::NOT_FILES)
            hx-vals=(vals) hx-target="this" hx-swap="outerHTML" { (inner) }
    }
}

/// The live region of the preview's short line, once per page after the
/// preview; `oob` when an answer updates it.
pub(super) fn said(line: &str, oob: bool) -> Markup {
    html! {
        div id="preview-said" class="visually-hidden" role="status" aria-live="polite"
            hx-swap-oob=[oob.then_some("innerHTML")] { (line) }
    }
}

/// What the preview of `f` shows: `empty` or `shown` (`long` is decided
/// before parsing).
fn state_of(f: &Fields) -> &'static str {
    if f.body_th.trim().is_empty() && f.body_en.trim().is_empty() {
        "empty"
    } else {
        "shown"
    }
}

/// The short line for a preview `state`.
fn line(v: &View<'_>, state: &str) -> String {
    v.t(match state {
        "long" => "announcements.editor.too_long",
        "empty" => "announcements.editor.preview_cleared",
        _ => "announcements.editor.preview_said",
    })
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
    let was = pairs
        .iter()
        .find(|(k, _)| k == "preview_was")
        .map(|(_, x)| x.clone());
    // One short line, only when what the preview shows changed.
    let tell = |state: &str, shown: Markup| {
        let changed = was.as_deref().is_some_and(|w| w != state);
        let out = html! { (shown) @if changed { (said(&line(&v, state), true)) } };
        (StatusCode::OK, Html(out.into_string())).into_response()
    };
    // The form's own limit, checked before anything is parsed. A 200 with
    // a line, not a 413: HTMX swaps no error status, so Staff would see
    // the old preview and nothing else.
    let long = |b: &str| b.chars().count() > MAX_BODY;
    if long(&f.body_th) || long(&f.body_en) {
        let shown = shell(
            "long",
            html! {
                // `#preview-said` announces it once (#156).
                p class="field-error" { (v.t("announcements.editor.too_long")) }
            },
        );
        return tell("long", shown);
    }
    // The edit form names its announcement: the preview shows its images.
    let id = pairs.iter().find(|(k, _)| k == "announcement_id");
    let images = match id {
        Some((_, id)) => files::list(&s.services, id).await.unwrap_or_default(),
        None => Vec::new(),
    };
    tell(state_of(&f), preview(&v, &f, &images))
}
