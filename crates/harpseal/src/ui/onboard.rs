//! Help where the work is (#127): the panel beside a file input that says
//! which columns and files an upload takes, the empty state of a list, and
//! the next step after a confirmed import.

use maud::{Markup, html};

use crate::{CurrentStaff, PageState};

/// Students a "แจ้งนักศึกษา" link names at most. The link is a URL, so a
/// longer list (or a query over [`MAX_PREFILL_QUERY`]) names no one and
/// Staff choose the Audience on the form. A limit of the link only: the
/// form takes any list Staff paste. With eight-digit codes the query cap
/// binds first, at about 790 codes.
pub const MAX_PREFILL: usize = 1000;
/// Longest query string a "แจ้งนักศึกษา" link carries, in bytes once
/// encoded: well inside the URL limits of browsers, proxies and
/// Cloudflare (16 KB).
pub const MAX_PREFILL_QUERY: usize = 7 * 1024;

/// One column as the help panel lists it.
pub(crate) struct HelpColumn {
    /// The header as it must appear in the file.
    pub(crate) header: String,
    /// What it holds, in Staff's words.
    pub(crate) holds: String,
    pub(crate) required: bool,
    /// An example value; empty for none.
    pub(crate) example: String,
}

/// The help panel's body (no card): the file rules, the columns and the
/// template link. `any_order` says the headers may come in any order (the
/// roster's reader); a Module's reader may not promise it.
pub(crate) fn upload_panel(
    s: &PageState,
    staff: &CurrentStaff,
    columns: &[HelpColumn],
    template: Option<&str>,
    any_order: bool,
) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let mb = (crate::MAX_UPLOAD_BYTES / (1024 * 1024)).to_string();
    let header_rule = if any_order {
        "kernel.upload.rule_header_any"
    } else {
        "kernel.upload.rule_header"
    };
    html! {
        div class="upload-help" {
            h2 { (t("kernel.upload.help_title")) }
            ul class="rules" {
                li { (s.catalog.tf(&staff.locale, "kernel.upload.rule_types", &[("mb", &mb)])) }
                li { (t("kernel.upload.rule_utf8")) }
                @if !columns.is_empty() { li { (t(header_rule)) } }
            }
            @if !columns.is_empty() {
                h3 { (t("kernel.upload.columns")) }
                ul class="list upload-columns" {
                    @for c in columns {
                        li {
                            span class="name code-head" { (c.header) }
                            @if c.required {
                                span class="chip" { (t("kernel.upload.required")) }
                            } @else {
                                span class="chip quiet" { (t("kernel.upload.optional")) }
                            }
                            // What it holds, unless the header already says
                            // it, and the example, on one line.
                            @let holds = (c.holds != c.header).then_some(c.holds.as_str());
                            @if holds.is_some() || !c.example.is_empty() {
                                span class="holds" {
                                    @if let Some(h) = holds { (h) }
                                    @if holds.is_some() && !c.example.is_empty() { " · " }
                                    @if !c.example.is_empty() {
                                        span class="muted" { (t("kernel.upload.example")) " " }
                                        span class="example" { (c.example) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if let Some(href) = template {
                p { a class="btn secondary" href=(href) download { (t("kernel.upload.template")) } }
            }
        }
    }
}

/// The help panel of the Module upload route registered at `path` (e.g.
/// `"/fees/upload"`), as a card to put beside the upload form:
///
/// ```text
/// div class="cols" {
///     form class="card" … { … input type="file" name="file" … }
///     (harpseal::ui::upload_help(&s, &staff, "/fees/upload"))
/// }
/// ```
///
/// It lists the file rules (CSV or XLSX, at most
/// [`crate::MAX_UPLOAD_BYTES`], CSV saved as UTF-8) and, when the Module
/// declared them with [`crate::ModuleRegistrar::sheet_columns`], each
/// column with its label, whether it may be empty and an example, and the
/// link to the CSV template (only for Staff holding the upload's
/// Permission).
pub fn upload_help(s: &PageState, staff: &CurrentStaff, path: &str) -> Markup {
    let declared = s.deployment.upload_columns(path).unwrap_or_default();
    let columns: Vec<HelpColumn> = declared
        .iter()
        .map(|c| HelpColumn {
            header: c.header.to_string(),
            holds: s.catalog.t(&staff.locale, c.label_key),
            required: c.required,
            example: c.example.to_string(),
        })
        .collect();
    // The template is served behind the upload's Permission: no link for
    // Staff who would get a 403.
    let may = s
        .deployment
        .back_office_permission(path)
        .is_some_and(|p| staff.can(p));
    let template = (may && !columns.is_empty()).then(|| crate::module::template_path(path));
    html! {
        aside class="card" aria-label=(s.catalog.t(&staff.locale, "kernel.upload.help_title")) {
            (upload_panel(s, staff, &columns, template.as_deref(), false))
        }
    }
}

/// An empty list (#127): one sentence on what goes here and, when the
/// Staff member may take it, the one action that fills it (check the
/// Permission before passing `action`, as `(label, href)`).
pub fn empty_state(text: &str, action: Option<(&str, &str)>) -> Markup {
    html! {
        div class="card empty" {
            p { (text) }
            @if let Some((label, href)) = action { a class="btn" href=(href) { (label) } }
        }
    }
}

/// A search or filter that matched nothing: what to try, and the link
/// that clears it (`(label, href)`).
pub fn no_match(text: &str, clear: (&str, &str)) -> Markup {
    html! {
        div class="card empty" {
            p { (text) }
            a class="btn secondary" href=(clear.1) { (clear.0) }
        }
    }
}

/// The new-announcement form for `students` (student codes, repeats
/// dropped in order): the Audience is those Students when there are 1 to
/// [`MAX_PREFILL`] of them and the query fits [`MAX_PREFILL_QUERY`], else
/// the bare form, where Staff choose. Nothing is sent from it without the
/// usual review.
pub fn announce_href(students: &[String]) -> String {
    prefill(students).unwrap_or_else(|| NEW_ANNOUNCEMENT.to_string())
}

const NEW_ANNOUNCEMENT: &str = "/admin/announcements/new";

/// The prefilled link, or `None` when it names no one.
fn prefill(students: &[String]) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    let codes: Vec<&String> = students.iter().filter(|c| seen.insert(*c)).collect();
    if codes.is_empty() || codes.len() > MAX_PREFILL {
        return None;
    }
    let list: Vec<String> = codes.iter().map(|c| query_escape(c)).collect();
    let query = format!("scope=students&students={}", list.join("+"));
    (query.len() <= MAX_PREFILL_QUERY).then(|| format!("{NEW_ANNOUNCEMENT}?{query}"))
}

/// How many different Students `students` names, if a link can carry them.
fn named(students: &[String]) -> Option<usize> {
    prefill(students)?;
    let distinct: std::collections::HashSet<&String> = students.iter().collect();
    Some(distinct.len())
}

fn query_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The step after a confirmed import (#127): "แจ้งนักศึกษา", a draft
/// announcement to `students` (student codes; see [`announce_href`]:
/// empty, or too many for a link, and Staff choose the Audience), for Staff who may send announcements, and the way back
/// (`(label, href)`). The draft goes through the usual review before
/// anything is sent.
pub fn next_step(
    s: &PageState,
    staff: &CurrentStaff,
    students: &[String],
    back: (&str, &str),
) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let may = staff.can(crate::modules::announcements::SEND_PERMISSION)
        && s.deployment.has_back_office("/announcements/new");
    let named = named(students);
    html! {
        section class="card next-step" id="next-step" {
            h2 { (t("kernel.next.title")) }
            @if may {
                p { @if let Some(n) = named {
                    (s.catalog.tf(&staff.locale, "kernel.next.named", &[("n", &crate::ui::number(n))]))
                } @else {
                    (t("kernel.next.choose"))
                } }
            }
            div class="actions" {
                @if may { a class="btn" href=(announce_href(students)) { (t("kernel.next.announce")) } }
                a class="btn secondary" href=(back.1) { (back.0) }
            }
        }
    }
}
