//! The Student picker's route (#146), mounted by a Module under its own
//! Permission with [`ModuleRegistrar::student_picker`]:
//!
//! - `GET ?q=&pf=&pmax=&{pf}`: up to [`RESULTS`] Students whose code
//!   starts with `q`, then those whose name contains it (names only for
//!   Staff holding `kernel.student.view`), each with "เพิ่ม" or, when in
//!   `{pf}` already, "เลือกแล้ว"; and one status line with the count. An
//!   HTMX fragment.
//! - `POST pf, pmax, op, code, paste, {pf}`: the picked codes in `{pf}`
//!   changed by `op` (`add`, `remove`, `paste`, `clear`), answered with the
//!   picked list again and one status line for the live region. Nothing
//!   is stored: the page's own form posts the codes.

use std::collections::HashMap;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use maud::{Markup, html};

use super::picker::{NAMES, PickerRoute, dedup, edit_button, field_ok, picked};
use crate::audience::{MAX_STUDENTS, parse_student_ids};
use crate::module::ModuleRegistrar;
use crate::sql_like::like;
use crate::{AppState, CurrentStaff, PageState, Result, Statement, Value};

/// Most Students one search lists.
pub(crate) const RESULTS: usize = 8;
/// Most characters of a search.
const MAX_QUERY: usize = 100;
/// Fewest characters before names are searched: a name search reads the
/// whole table, so one letter (which matches almost everyone) does not.
const MIN_NAME_QUERY: usize = 2;
/// Codes per lookup query: D1 binds at most 100 values.
const CHUNK: usize = 90;

impl ModuleRegistrar {
    /// Mounts the route a [`crate::ui::StudentPicker`] searches and edits
    /// through at `/admin{route.path()}`, for Staff holding `permission`
    /// only (the guard's 403 for others; a POST needs the CSRF token HTMX
    /// sends). Give [`crate::ui::StudentPicker::new`] the same
    /// [`PickerRoute`] constant. Names are searched and shown only to Staff
    /// who also hold `kernel.student.view`.
    pub fn student_picker(&mut self, permission: &'static str, route: PickerRoute) -> &mut Self {
        self.back_office(permission, route.path(), get(search).post(edit))
    }
}

/// A picked code as the database knows it.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    /// The display name: `None` unless asked for (Staff holding [`NAMES`]).
    pub(crate) name: Option<String>,
    /// The Student has left.
    pub(crate) left: bool,
}

/// Picked codes that exist, by code; a code missing here is unknown.
pub(crate) type Resolved = HashMap<String, Known>;

/// Looks up `codes`, [`CHUNK`] at a time; their names only with `names`
/// (else the name is never read).
pub(crate) async fn resolve(s: &PageState, codes: &[String], names: bool) -> Result<Resolved> {
    let mut out = Resolved::new();
    let name = if names {
        "display_name"
    } else {
        "NULL AS display_name"
    };
    for chunk in codes.chunks(CHUNK) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let mut q = Statement::new(format!(
            "SELECT student_code, {name}, status FROM student \
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

/// One search hit: (code, name, left).
type Hit = (String, String, bool);

/// Students whose code starts with `q`, then (with `names`, and `q` of
/// at least [`MIN_NAME_QUERY`] characters) those whose name contains it;
/// each part active first, by code: at most [`RESULTS`] and whether there
/// are more.
///
/// The code part is a range on the code's unique index (`>= q` and below
/// `q` followed by the last code point), so it reads only the matching
/// rows; it is case-sensitive, as codes are. A name can match anywhere, so
/// that part reads the table, but stops once enough rows are found.
pub(crate) async fn find(s: &PageState, q: &str, names: bool) -> Result<(Vec<Hit>, bool)> {
    let want = (RESULTS + 1) as i64;
    let codes = Statement::new(
        "SELECT student_code, display_name, status FROM student \
         WHERE student_code >= ? AND student_code < ? || char(1114111) \
         ORDER BY status = 'left', student_code LIMIT ?",
    )
    .bind(q.to_string())
    .bind(q.to_string())
    .bind(want);
    let mut hits = rows_to_hits(&s.services.db.query(codes).await?);
    if names && hits.len() <= RESULTS && q.chars().count() >= MIN_NAME_QUERY {
        // Rows the code part found are skipped here, not in SQL: at most
        // RESULTS of them.
        let by_name = Statement::new(
            "SELECT student_code, display_name, status FROM student \
             WHERE display_name LIKE ? ESCAPE '\\' \
             ORDER BY status = 'left', student_code LIMIT ?",
        )
        .bind(like(q, true))
        .bind(want + hits.len() as i64);
        for h in rows_to_hits(&s.services.db.query(by_name).await?) {
            if !hits.iter().any(|x| x.0 == h.0) {
                hits.push(h);
            }
        }
    }
    let more = hits.len() > RESULTS;
    hits.truncate(RESULTS);
    Ok((hits, more))
}

fn rows_to_hits(rows: &crate::Rows) -> Vec<Hit> {
    (0..rows.len())
        .map(|i| {
            let t = |c: &str| {
                let v = rows.get(i, c).and_then(Value::as_text);
                v.unwrap_or_default().to_string()
            };
            (t("student_code"), t("display_name"), t("status") == "left")
        })
        .collect()
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
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    let Some((f, _)) = field_of(&pairs) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let q: String = value(&pairs, "q").trim().chars().take(MAX_QUERY).collect();
    if q.is_empty() {
        let clear = html! { (status_line(&f, "")) };
        return Html(clear.into_string()).into_response();
    }
    let names = staff.can(NAMES);
    let (hits, more) = match find(&s, &q, names).await {
        Ok(found) => found,
        Err(e) => return super::service_error(&s, &staff, &e),
    };
    let chosen = parse_student_ids(value(&pairs, &f));
    Html(results(&s, &staff, &f, &hits, more, names, &chosen).into_string()).into_response()
}

/// The status line's new text, swapped in out of band: the one thing a
/// screen reader reads after a search or a change.
fn status_line(f: &str, text: &str) -> Markup {
    html! { p id={ (f) "-status" } hx-swap-oob="innerHTML" { (text) } }
}

fn results(
    s: &PageState,
    staff: &CurrentStaff,
    f: &str,
    hits: &[Hit],
    more: bool,
    names: bool,
    chosen: &[String],
) -> Markup {
    let tf = |k: &str, a: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, a);
    let t = |k: &str| tf(k, &[]);
    let n = super::number(hits.len() as i64);
    let status = match (hits.is_empty(), more) {
        (true, _) => t("kernel.picker.none"),
        (false, false) => tf("kernel.picker.found", &[("n", &n)]),
        (false, true) => tf("kernel.picker.found_more", &[("n", &n)]),
    };
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
                        @if chosen.contains(code) {
                            " " span class="picker-picked" { (t("kernel.picker.picked")) }
                        } @else {
                            (edit_button("add", code, &t("kernel.picker.add"),
                                &tf("kernel.picker.add_aria", &[("code", code)]), "secondary"))
                        }
                    }
                }
            }
            @if more {
                p class="muted" { (tf("kernel.picker.more", &[("n", &RESULTS.to_string())])) }
            }
        }
        (status_line(f, &status))
    }
}

async fn edit(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let Some((f, max)) = field_of(&pairs) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // What the page had, as the page could have kept it.
    let before = dedup(&parse_student_ids(value(&pairs, &f)), MAX_STUDENTS);
    let mut codes = before.clone();
    let code = parse_student_ids(value(&pairs, "code")).into_iter().next();
    // New codes asked for, before the cap.
    let mut asked = 0;
    let (mut key, arg) = match (value(&pairs, "op"), code) {
        ("add", Some(c)) if before.contains(&c) => ("kernel.picker.already", c),
        ("add", Some(c)) => {
            codes.push(c.clone());
            asked = 1;
            ("kernel.picker.added", c)
        }
        ("remove", Some(c)) => {
            codes.retain(|x| *x != c);
            ("kernel.picker.removed", c)
        }
        ("paste", _) => {
            let new: Vec<String> = parse_student_ids(value(&pairs, "paste"))
                .into_iter()
                .filter(|c| !before.contains(c))
                .collect();
            asked = new.len();
            codes.extend(new);
            ("kernel.picker.pasted", String::new())
        }
        ("clear", _) => {
            codes.clear();
            ("kernel.picker.cleared", String::new())
        }
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let codes = dedup(&codes, MAX_STUDENTS);
    // Only the codes kept under the cap count as added.
    let added = codes.len().saturating_sub(before.len());
    if key == "kernel.picker.added" && added == 0 {
        key = "kernel.picker.full";
    }
    let names = staff.can(NAMES);
    let found = match resolve(&s, &codes, names).await {
        Ok(found) => found,
        Err(e) => return super::service_error(&s, &staff, &e),
    };
    let tf = |k: &str, a: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, a);
    let n = super::number(codes.len() as i64);
    let cap = super::number(MAX_STUDENTS as i64);
    let added_s = super::number(added as i64);
    let mut status = tf(
        key,
        &[
            ("code", &arg),
            ("n", &n),
            ("added", &added_s),
            ("max", &cap),
        ],
    );
    if key == "kernel.picker.pasted" && added < asked {
        status = format!("{status} {}", tf("kernel.picker.full_n", &[("max", &cap)]));
    }
    let unknown = codes.iter().filter(|c| !found.contains_key(*c)).count();
    if unknown > 0 && matches!(key, "kernel.picker.added" | "kernel.picker.pasted") {
        let u = super::number(unknown as i64);
        status = format!("{status} {}", tf("kernel.picker.unknown_n", &[("n", &u)]));
    }
    let body = html! {
        (picked(&s, &staff, &f, max, &codes, &found, false))
        (status_line(&f, &status))
        @if key == "kernel.picker.pasted" {
            textarea id={ (f) "-paste" } name="paste" form={ (f) "-detached" } rows="4"
                aria-labelledby={ (f) "-paste-label" } hx-swap-oob="true" {}
        }
    };
    Html(body.into_string()).into_response()
}
