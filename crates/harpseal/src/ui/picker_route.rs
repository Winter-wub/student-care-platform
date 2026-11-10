//! The Student picker's route (#146), mounted by a Module under its own
//! Permission with [`ModuleRegistrar::student_picker`]:
//!
//! - `GET ?q=&pf=&pmax=`: up to [`RESULTS`] Students whose code starts
//!   with `q`, or whose name contains it (names only for Staff holding
//!   `kernel.student.view`), each with "เพิ่ม". An HTMX fragment.
//! - `POST pf, pmax, op, code, paste, {pf}`: the picked codes in `{pf}`
//!   changed by `op` (`add`, `remove`, `paste`, `clear`), answered with the
//!   picked list again and one status line for the live region. Nothing
//!   is stored: the page's own form posts the codes.

use std::collections::HashMap;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use maud::{Markup, html};

use super::picker::{NAMES, dedup, edit_button, field_ok, picked};
use crate::audience::{MAX_STUDENTS, parse_student_ids};
use crate::module::ModuleRegistrar;
use crate::{AppState, CurrentStaff, PageState, Result, Statement, Value};

/// Most Students one search lists.
pub(crate) const RESULTS: usize = 8;
/// Most characters of a search.
const MAX_QUERY: usize = 100;
/// Longest `LIKE` pattern in bytes: Cloudflare D1 refuses more than 50.
const MAX_LIKE: usize = 50;
/// Codes per lookup query: D1 binds at most 100 values.
const CHUNK: usize = 90;

impl ModuleRegistrar {
    /// Mounts the route a [`crate::ui::StudentPicker`] searches and edits
    /// through at `/admin{path}` (e.g. `path = "/reminders/pick"`), for
    /// Staff holding `permission` only (the guard's 403 for others; a POST
    /// needs the CSRF token HTMX sends). Use the same `path` in
    /// [`crate::ui::StudentPicker::new`]. Names are searched and shown
    /// only to Staff who also hold `kernel.student.view`.
    pub fn student_picker(&mut self, permission: &'static str, path: &'static str) -> &mut Self {
        self.back_office(permission, path, get(search).post(edit))
    }
}

/// A picked code as the database knows it.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    /// The display name (shown only to Staff holding [`NAMES`]).
    pub(crate) name: Option<String>,
    /// The Student has left.
    pub(crate) left: bool,
}

/// Picked codes that exist, by code; a code missing here is unknown.
pub(crate) type Resolved = HashMap<String, Known>;

/// Looks up `codes`, [`CHUNK`] at a time.
pub(crate) async fn resolve(s: &PageState, codes: &[String]) -> Result<Resolved> {
    let mut out = Resolved::new();
    for chunk in codes.chunks(CHUNK) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let mut q = Statement::new(format!(
            "SELECT student_code, display_name, status FROM student \
             WHERE student_code IN ({marks})"
        ));
        for c in chunk {
            q = q.bind(c.clone());
        }
        let rows = s.services.db.query(q).await?;
        for i in 0..rows.len() {
            let text = |c: &str| rows.get(i, c).and_then(Value::as_text).map(str::to_string);
            if let Some(code) = text("student_code") {
                let left = text("status").as_deref() == Some("left");
                out.insert(
                    code,
                    Known {
                        name: text("display_name"),
                        left,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// `q` escaped for `LIKE` (`\`), `lead` before it and `%` after, cut
/// after the last whole character that keeps it within [`MAX_LIKE`].
pub(crate) fn like(lead: &str, q: &str) -> String {
    let mut p = String::from(lead);
    for c in q.chars() {
        let escape = matches!(c, '%' | '_' | '\\');
        if p.len() + usize::from(escape) + c.len_utf8() + 1 > MAX_LIKE {
            break;
        }
        if escape {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// One search hit: (code, name, left).
type Hit = (String, String, bool);

/// Students whose code starts with `q` (or, with `names`, whose name
/// contains it), active first, by code: at most [`RESULTS`] and whether
/// there are more.
pub(crate) async fn find(s: &PageState, q: &str, names: bool) -> Result<(Vec<Hit>, bool)> {
    let sql = format!(
        "SELECT student_code, display_name, status FROM student \
         WHERE student_code LIKE ? ESCAPE '\\' {} \
         ORDER BY status = 'left', student_code LIMIT ?",
        if names {
            "OR display_name LIKE ? ESCAPE '\\'"
        } else {
            ""
        }
    );
    let mut st = Statement::new(sql).bind(like("", q));
    if names {
        st = st.bind(like("%", q));
    }
    let rows = s.services.db.query(st.bind((RESULTS + 1) as i64)).await?;
    let mut hits: Vec<Hit> = (0..rows.len())
        .map(|i| {
            let t = |c: &str| {
                let v = rows.get(i, c).and_then(Value::as_text);
                v.unwrap_or_default().to_string()
            };
            (t("student_code"), t("display_name"), t("status") == "left")
        })
        .collect();
    let more = hits.len() > RESULTS;
    hits.truncate(RESULTS);
    Ok((hits, more))
}

fn value<'a>(pairs: &'a [(String, String)], k: &str) -> &'a str {
    pairs.iter().find(|(n, _)| n == k).map_or("", |(_, v)| v)
}

/// The field name and the limit the picker sent, or `None` when the
/// name is unusable.
fn field_of(pairs: &[(String, String)]) -> Option<(String, usize)> {
    let f = value(pairs, "pf");
    let max = value(pairs, "pmax")
        .parse::<usize>()
        .unwrap_or(MAX_STUDENTS);
    field_ok(f).then(|| (f.to_string(), max.clamp(1, MAX_STUDENTS)))
}

async fn search(
    State(s): State<AppState>,
    staff: CurrentStaff,
    uri: Uri,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    let Some((f, _)) = field_of(&pairs) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let q: String = value(&pairs, "q").trim().chars().take(MAX_QUERY).collect();
    if q.is_empty() {
        return Html(String::new()).into_response();
    }
    let names = staff.can(NAMES);
    let (hits, more) = match find(&s, &q, names).await {
        Ok(found) => found,
        Err(e) => return super::service_error(&s, &staff, &e),
    };
    Html(results(&s, &staff, uri.path(), &f, &hits, more, names).into_string()).into_response()
}

fn results(
    s: &PageState,
    staff: &CurrentStaff,
    url: &str,
    f: &str,
    hits: &[Hit],
    more: bool,
    names: bool,
) -> Markup {
    let tf = |k: &str, a: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, a);
    let t = |k: &str| tf(k, &[]);
    html! {
        @if hits.is_empty() {
            p class="muted" { (t("kernel.picker.none")) }
        } @else {
            ul class="list picker-found" {
                @for (code, name, left) in hits {
                    li {
                        span class="picker-code" { (code) }
                        @if names { " " span class="picker-name" { (name) } }
                        @if *left { " " span class="chip warn" { (t("kernel.picker.left")) } }
                        (edit_button(url, f, "add", code, &t("kernel.picker.add"),
                            &tf("kernel.picker.add_aria", &[("code", code)]), "secondary"))
                    }
                }
            }
            @if more {
                p class="muted" { (tf("kernel.picker.more", &[("n", &RESULTS.to_string())])) }
            }
        }
    }
}

async fn edit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    uri: Uri,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let Some((f, max)) = field_of(&pairs) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut codes = parse_student_ids(value(&pairs, &f));
    let code = parse_student_ids(value(&pairs, "code")).into_iter().next();
    let before = codes.len();
    let (key, arg) = match (value(&pairs, "op"), code) {
        ("add", Some(c)) => {
            codes.push(c.clone());
            ("kernel.picker.added", c)
        }
        ("remove", Some(c)) => {
            codes.retain(|x| *x != c);
            ("kernel.picker.removed", c)
        }
        ("paste", _) => {
            codes.extend(parse_student_ids(value(&pairs, "paste")));
            ("kernel.picker.pasted", String::new())
        }
        ("clear", _) => {
            codes.clear();
            ("kernel.picker.cleared", String::new())
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let codes = dedup(&codes, MAX_STUDENTS);
    let found = match resolve(&s, &codes).await {
        Ok(found) => found,
        Err(e) => return super::service_error(&s, &staff, &e),
    };
    let tf = |k: &str, a: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, a);
    let n = super::number(codes.len() as i64);
    let added = super::number(codes.len().saturating_sub(before) as i64);
    let mut status = tf(key, &[("code", &arg), ("n", &n), ("added", &added)]);
    let unknown = codes.iter().filter(|c| !found.contains_key(*c)).count();
    if unknown > 0 && matches!(key, "kernel.picker.added" | "kernel.picker.pasted") {
        let u = super::number(unknown as i64);
        status = format!("{status} {}", tf("kernel.picker.unknown_n", &[("n", &u)]));
    }
    let body = html! {
        (picked(&s, &staff, uri.path(), &f, max, &codes, &found, false))
        p id={ (f) "-status" } hx-swap-oob="innerHTML" { (status) }
        @if key == "kernel.picker.pasted" {
            textarea id={ (f) "-paste" } name="paste" form={ (f) "-detached" } rows="4"
                aria-labelledby={ (f) "-paste-label" } hx-swap-oob="true" {}
        }
    };
    Html(body.into_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::like;

    #[test]
    fn a_search_is_escaped_and_fits_d1() {
        assert_eq!(like("", "6801"), "6801%");
        assert_eq!(like("%", "a_b%c\\"), "%a\\_b\\%c\\\\%");
        let long = like("%", &"ก".repeat(40));
        assert!(long.len() <= 50 && long.ends_with('%'), "{long}");
    }
}
