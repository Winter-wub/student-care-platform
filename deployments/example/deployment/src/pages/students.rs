//! `/admin/students`: list with HTMX search, and the announcement form.

use axum::Form;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse};
use harpseal::AppState;
use harpseal::port_types::{Rows, Statement};
use harpseal::ui::{Flash, Shell, flash, page as layout};
use maud::{Markup, html};
use serde::Deserialize;

use super::{Page, Staff, int, is_htmx, text, uuid7};

#[derive(Deserialize, Default)]
pub struct Search {
    #[serde(default)]
    q: String,
}

fn list_query(q: &str) -> Statement {
    let like = format!("%{}%", q.trim());
    Statement::new(
        "SELECT student_code, display_name, json_extract(attributes, '$.programme'), \
         json_extract(attributes, '$.year'), status FROM student \
         WHERE display_name LIKE ? OR student_code LIKE ? ORDER BY student_code LIMIT 100",
    )
    .bind(like.clone())
    .bind(like)
}

fn rows_fragment(rows: &Rows, q: &str) -> Markup {
    html! {
        p class="muted count" {
            @if q.trim().is_empty() { "ทั้งหมด " (rows.rows.len()) " คน" }
            @else { "พบ " (rows.rows.len()) " คน สำหรับ “" (q.trim()) "”" }
        }
        ul class="list" {
            @for r in &rows.rows {
                li {
                    span class="name" { (text(r, 1)) br; span class="code" { (text(r, 0)) } }
                    span class="chip" { (text(r, 2)) }
                    span class="chip" { "ปี " (int(r, 3)) }
                    @if text(r, 4) != "active" { span class="chip" { "พ้นสภาพ" } }
                }
            }
        }
    }
}

/// `GET /admin/students?q=`: the whole page.
pub async fn page(staff: Staff, State(s): State<AppState>, Query(search): Query<Search>) -> Page {
    let rows = s.services.db.query(list_query(&search.q)).await?;
    let body = html! {
        div class="cols" {
            section class="card" {
                h1 { "นักศึกษา" }
                div class="search" {
                    label for="q" { "ค้นหาชื่อหรือรหัส" }
                    input id="q" type="search" name="q" value=(search.q) placeholder="เช่น สมใจ หรือ 6801"
                        autocomplete="off" inputmode="search"
                        hx-get="/admin/students/rows" hx-trigger="input changed delay:250ms, search"
                        hx-target="#rows" hx-sync="this:replace";
                }
                div id="rows" { (rows_fragment(&rows, &search.q)) }
            }
            (announce_card(None, &Announce::default()))
        }
    };
    let html = layout(
        &s.org,
        "นักศึกษา",
        Shell::BackOffice {
            staff_email: Some(&staff.email),
        },
        body,
    );
    Ok(Html(html.into_string()).into_response())
}

/// `GET /admin/students/rows?q=`: the HTMX fragment.
pub async fn rows(_: Staff, State(s): State<AppState>, Query(q): Query<Search>) -> Page {
    let rows = s.services.db.query(list_query(&q.q)).await?;
    Ok(Html(rows_fragment(&rows, &q.q).into_string()).into_response())
}

const AUDIENCES: &[(&str, &str)] = &[
    ("all", "นักศึกษาทุกคน"),
    ("year:1", "ชั้นปีที่ 1"),
    ("year:2", "ชั้นปีที่ 2"),
    ("year:3", "ชั้นปีที่ 3"),
    ("year:4", "ชั้นปีที่ 4"),
    ("year:5", "ชั้นปีที่ 5"),
    ("year:6", "ชั้นปีที่ 6"),
    ("programme:DDS", "หลักสูตร DDS"),
    ("programme:DDS-Intl", "หลักสูตร DDS-Intl"),
];

/// Audience key -> count statement. Only keys from `AUDIENCES` pass.
fn audience_count_query(key: &str) -> Option<Statement> {
    if !AUDIENCES.iter().any(|(k, _)| *k == key) {
        return None;
    }
    match key.split_once(':') {
        None => Some(Statement::new(
            "SELECT count(*) FROM student WHERE status = 'active'",
        )),
        Some(("year", y)) => y.parse::<i64>().ok().map(|y| {
            Statement::new(
                "SELECT count(*) FROM student WHERE status = 'active' \
                 AND json_extract(attributes, '$.year') = ?",
            )
            .bind(y)
        }),
        Some(("programme", p)) => Some(
            Statement::new(
                "SELECT count(*) FROM student WHERE status = 'active' \
                 AND json_extract(attributes, '$.programme') = ?",
            )
            .bind(p),
        ),
        _ => None,
    }
}

#[derive(Deserialize, Default)]
pub struct Announce {
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    require_ack: Option<String>,
}

fn announce_card(result: Option<Flash<'_>>, v: &Announce) -> Markup {
    let selected = if v.audience.is_empty() {
        "all"
    } else {
        &v.audience
    };
    html! {
        section class="card" id="announce" {
            h2 { "ส่งประกาศ" }
            @if let Some(f) = &result { (flash(f)) }
            form method="post" action="/admin/announcements"
                hx-post="/admin/announcements" hx-target="#announce" hx-swap="outerHTML" {
                div class="field" {
                    label for="title" { "หัวข้อ" }
                    input id="title" name="title" required maxlength="120" value=(v.title);
                }
                div class="field" {
                    label for="body" { "ข้อความ" }
                    textarea id="body" name="body" required maxlength="2000" { (v.body) }
                }
                div class="field" {
                    label for="audience" { "ผู้รับ" }
                    select id="audience" name="audience"
                        hx-get="/admin/announcements/audience" hx-trigger="change, load"
                        hx-target="#audience-count" {
                        @for (k, label) in AUDIENCES {
                            option value=(k) selected[*k == selected] { (label) }
                        }
                    }
                    p class="muted" id="audience-count" {}
                }
                div class="field" {
                    label { input type="checkbox" name="require_ack" value="1" checked style="width:auto;min-height:0"; " ขอให้กด “รับทราบ”" }
                }
                button class="wide" type="submit" { "บันทึกประกาศ" }
            }
        }
    }
}

/// `GET /admin/announcements/audience?audience=`: live count.
pub async fn audience_count(
    _: Staff,
    State(s): State<AppState>,
    Query(a): Query<Announce>,
) -> Page {
    let Some(q) = audience_count_query(&a.audience) else {
        return Ok(Html("เลือกผู้รับ").into_response());
    };
    let n = s.services.db.query(q).await?;
    let n = n.rows.first().map(|r| int(r, 0)).unwrap_or(0);
    Ok(Html(format!("จะส่งถึง {n} คน")).into_response())
}

/// `POST /admin/announcements`: writes one `announcement` row (+ its Thai
/// text) in one batch and shows a flash. Nothing is sent to a Channel.
pub async fn announce(
    staff: Staff,
    State(s): State<AppState>,
    headers: HeaderMap,
    Form(v): Form<Announce>,
) -> Page {
    let (title, body) = (v.title.trim(), v.body.trim());
    let error = if title.is_empty() || body.is_empty() {
        Some("กรอกหัวข้อและข้อความ")
    } else if title.chars().count() > 120 || body.chars().count() > 2000 {
        Some("หัวข้อยาวได้ 120 ตัวอักษร ข้อความ 2000 ตัวอักษร")
    } else if audience_count_query(&v.audience).is_none() {
        Some("เลือกผู้รับ")
    } else {
        None
    };
    let card = if let Some(e) = error {
        announce_card(Some(Flash { ok: false, text: e }), &v)
    } else {
        let id = uuid7(&s.services)?;
        let now = s.services.clock.now().0;
        let audience = format!("{{\"key\":\"{}\"}}", v.audience);
        s.services
            .db
            .batch(vec![
                Statement::new(
                    "INSERT INTO announcement (id, status, audience, require_ack, created_by, created_at) \
                     VALUES (?, 'draft', ?, ?, ?, ?)",
                )
                .bind(id.clone())
                .bind(audience)
                .bind(i64::from(v.require_ack.is_some()))
                .bind(staff.id.clone())
                .bind(now),
                Statement::new(
                    "INSERT INTO announcement_text (announcement_id, locale, title, body) VALUES (?, 'th', ?, ?)",
                )
                .bind(id)
                .bind(title)
                .bind(body),
            ])
            .await?;
        let text = "บันทึกประกาศแล้ว (ต้นแบบ: ยังไม่ส่งออกช่องทางจริง) ดูได้ในหน้า Mini App";
        announce_card(Some(Flash { ok: true, text }), &Announce::default())
    };
    if is_htmx(&headers) {
        return Ok(Html(card.into_string()).into_response());
    }
    let html = layout(
        &s.org,
        "ส่งประกาศ",
        Shell::BackOffice {
            staff_email: Some(&staff.email),
        },
        card,
    );
    Ok(Html(html.into_string()).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_audiences_pass() {
        assert!(audience_count_query("all").is_some());
        assert!(audience_count_query("year:3").is_some());
        assert!(audience_count_query("programme:DDS-Intl").is_some());
        assert!(audience_count_query("programme:x' OR 1=1").is_none());
        assert!(audience_count_query("").is_none());
    }
}
