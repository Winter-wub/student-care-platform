//! Mini App page (`/app`), its acknowledge button, the public notice and a
//! small index. Prototype: the Student comes from `?student=<code>` or a
//! cookie, not from a LIFF id token / Telegram `initData` App Session.

use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse};
use harpseal::AppState;
use harpseal::port_types::{Rows, Statement};
use harpseal::ui::{Shell, page as layout};
use maud::{Markup, html};
use serde::Deserialize;

use super::{Page, SecureCookies, cookie, int, text};

const STUDENT_COOKIE: &str = "hs_demo_student";

#[derive(Deserialize)]
pub struct Who {
    student: Option<String>,
}

/// `Thai short date` in Asia/Bangkok, Buddhist Era: "6 พ.ย. 2569".
fn thai_date(ms: i64) -> String {
    const MONTHS: [&str; 12] = [
        "ม.ค.",
        "ก.พ.",
        "มี.ค.",
        "เม.ย.",
        "พ.ค.",
        "มิ.ย.",
        "ก.ค.",
        "ส.ค.",
        "ก.ย.",
        "ต.ค.",
        "พ.ย.",
        "ธ.ค.",
    ];
    let days = (ms + 7 * 3_600_000).div_euclid(86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d} {} {}", MONTHS[(m - 1) as usize], y + 543)
}

fn ack_button(id: &str, acked: bool) -> Markup {
    html! {
        @if acked {
            button class="done" hx-post={"/app/ack/" (id)} hx-swap="outerHTML" aria-pressed="true" { "✓ รับทราบแล้ว" }
        } @else {
            button hx-post={"/app/ack/" (id)} hx-swap="outerHTML" aria-pressed="false" { "รับทราบ" }
        }
    }
}

async fn find_student(s: &AppState, code: &str) -> harpseal::Result<Option<Vec<String>>> {
    let q = Statement::new(
        "SELECT id, student_code, display_name, json_extract(attributes, '$.programme'), \
         json_extract(attributes, '$.year') FROM student WHERE student_code = ? AND status = 'active'",
    )
    .bind(code);
    let rows = s.services.db.query(q).await?;
    Ok(rows
        .rows
        .first()
        .map(|r| (0..5).map(|i| text(r, i)).collect()))
}

fn news(rows: &Rows) -> Markup {
    html! {
        ul class="list news" {
            @for r in &rows.rows {
                li {
                    h3 { (text(r, 1)) }
                    p { (text(r, 2)) }
                    div class="meta" {
                        span class="muted" { (thai_date(int(r, 3))) }
                        @if int(r, 4) == 1 { (ack_button(&text(r, 0), int(r, 5) > 0)) }
                    }
                }
            }
        }
    }
}

/// `GET /app?student=<code>`.
pub async fn page(
    State(s): State<AppState>,
    Extension(SecureCookies(secure)): Extension<SecureCookies>,
    Query(who): Query<Who>,
    headers: HeaderMap,
) -> Page {
    let from_query = who.student.filter(|c| !c.is_empty() && c.len() <= 20);
    let code = from_query
        .clone()
        .or_else(|| cookie(&headers, STUDENT_COOKIE).map(str::to_string));
    let student = match &code {
        Some(c) => find_student(&s, c).await?,
        None => None,
    };
    let body = match &student {
        None => html! {
            div class="card" {
                h1 { "ยังไม่ได้ยืนยันตัวตน" }
                p { "กลับไปที่แชทแล้วแตะ “เริ่มใช้งาน” เพื่อยืนยันว่าเป็นนักศึกษา จากนั้นเปิดหน้านี้อีกครั้ง" }
                p class="muted" { "ต้นแบบ: เปิด /app?student=68010001 เพื่อดูหน้าของนักศึกษาสมมติ" }
            }
        },
        Some(st) => {
            let q = Statement::new(
                "SELECT a.id, t.title, t.body, a.created_at, a.require_ack, \
                 (SELECT count(*) FROM announcement_ack k WHERE k.announcement_id = a.id AND k.student_id = ?) \
                 FROM announcement a JOIN announcement_text t ON t.announcement_id = a.id AND t.locale = 'th' \
                 ORDER BY a.created_at DESC LIMIT 5",
            )
            .bind(st[0].clone());
            let rows = s.services.db.query(q).await?;
            let initial: String = st[2].chars().next().map(String::from).unwrap_or_default();
            html! {
                section class="card" {
                    h2 { "สถานะการเชื่อมบัญชี" }
                    div class="status" {
                        div class="avatar" aria-hidden="true" { (initial) }
                        div {
                            div class="name" { (st[2]) }
                            div class="code" { "รหัส " (st[1]) " · " (st[3]) " ปี " (st[4]) }
                        }
                    }
                    p style="margin-top:12px" { span class="chip ok" { "เชื่อมกับ LINE แล้ว" } " " span class="chip" { "Telegram ยังไม่เชื่อม" } }
                }
                section class="card" {
                    h2 { "ประกาศล่าสุด" }
                    (news(&rows))
                }
            }
        }
    };
    let html = layout(&s.org, "Mini App", Shell::MiniApp, body);
    let mut resp = Html(html.into_string()).into_response();
    if let Some(c) = from_query {
        // SameSite=None so the cookie also works when Telegram Desktop/Web
        // opens the Web App in an iframe; it needs Secure.
        let attrs = if secure {
            "SameSite=None; Secure"
        } else {
            "SameSite=Lax"
        };
        let v = format!("{STUDENT_COOKIE}={c}; Path=/app; HttpOnly; Max-Age=3600; {attrs}");
        if let Ok(v) = HeaderValue::from_str(&v) {
            resp.headers_mut().insert(header::SET_COOKIE, v);
        }
    }
    Ok(resp)
}

/// `POST /app/ack/{id}`: toggles the acknowledgement, returns the button.
pub async fn ack(State(s): State<AppState>, Path(id): Path<String>, headers: HeaderMap) -> Page {
    let student = match cookie(&headers, STUDENT_COOKIE) {
        Some(c) => find_student(&s, c).await?,
        None => None,
    };
    let Some(st) = student else {
        return Ok((StatusCode::UNAUTHORIZED, "ยังไม่ได้ยืนยันตัวตน").into_response());
    };
    let now = s.services.clock.now().0;
    let insert = Statement::new(
        "INSERT INTO announcement_ack (announcement_id, student_id, acked_at) \
         SELECT id, ?, ? FROM announcement WHERE id = ? ON CONFLICT DO NOTHING",
    )
    .bind(st[0].clone())
    .bind(now)
    .bind(id.clone());
    let acked = s.services.db.execute(insert).await?.rows_affected > 0;
    if !acked {
        let del = Statement::new(
            "DELETE FROM announcement_ack WHERE announcement_id = ? AND student_id = ?",
        )
        .bind(id.clone())
        .bind(st[0].clone());
        s.services.db.execute(del).await?;
    }
    Ok(Html(ack_button(&id, acked).into_string()).into_response())
}

/// `GET /notice`: the public privacy notice (fictional text).
pub async fn notice(State(s): State<AppState>) -> Page {
    let body = html! {
        article class="card notice" {
            h1 { "ประกาศความเป็นส่วนตัว" }
            p class="muted" { "ฉบับที่ 1 · มีผลตั้งแต่ 1 พฤศจิกายน 2569" }
            p { (s.org) " (“มหาวิทยาลัย”) ใช้ระบบนี้เพื่อส่งข่าวและตอบคำถามนักศึกษาผ่าน LINE และ Telegram ประกาศนี้อธิบายว่าเราเก็บข้อมูลอะไร ใช้ทำอะไร และคุณมีสิทธิ์อะไรบ้าง" }
            h2 { "ข้อมูลที่เราเก็บ" }
            ul {
                li { "รหัสนักศึกษา ชื่อ-สกุล อีเมล ชั้นปี และหลักสูตร จากทะเบียนนักศึกษา" }
                li { "รหัสผู้ใช้ LINE หรือ Telegram ของคุณ เมื่อคุณยืนยันตัวตนกับบอท" }
                li { "ข้อความที่คุณส่งถึงบอท และการกด “รับทราบ” ประกาศ" }
            }
            h2 { "เราใช้ข้อมูลเพื่อ" }
            ul {
                li { "ส่งประกาศให้ตรงกลุ่ม เช่น ตามชั้นปีหรือหลักสูตร" }
                li { "ให้เจ้าหน้าที่ตอบคำถามของคุณได้ถูกคน" }
                li { "ดูว่าประกาศสำคัญไปถึงนักศึกษาครบหรือยัง" }
            }
            h2 { "สิทธิ์ของคุณ" }
            p { "คุณขอดูหรือแก้ไขข้อมูลของตัวเองได้ และยกเลิกการเชื่อมบัญชีได้ทุกเมื่อ โดยติดต่อกองกิจการนักศึกษา เราไม่ขายหรือส่งต่อข้อมูลของคุณให้บุคคลภายนอก" }
            p class="muted" { "ข้อความนี้เป็นตัวอย่างสำหรับต้นแบบ ไม่ใช่ประกาศจริงของสถาบันใด" }
        }
    };
    let html = layout(&s.org, "ประกาศความเป็นส่วนตัว", Shell::MiniApp, body);
    Ok(Html(html.into_string()).into_response())
}

/// `GET /`: links to the prototype pages.
pub async fn index(State(s): State<AppState>) -> Html<String> {
    let body = html! {
        div class="card" {
            h1 { (s.org) " · ต้นแบบ" }
            ul class="list" {
                li { a class="name" href="/admin/login" { "หลังบ้าน: เข้าสู่ระบบเจ้าหน้าที่" } }
                li { a class="name" href="/app?student=68010001" { "Mini App: นักศึกษาสมมติ 68010001" } }
                li { a class="name" href="/notice" { "ประกาศความเป็นส่วนตัว" } }
            }
        }
    };
    Html(layout(&s.org, "ต้นแบบ", Shell::MiniApp, body).into_string())
}

#[cfg(test)]
mod tests {
    use super::thai_date;

    #[test]
    fn bangkok_buddhist_era_dates() {
        // 2026-11-05T17:30:00Z is 6 Nov 00:30 in Bangkok.
        assert_eq!(thai_date(1_793_899_800_000), "6 พ.ย. 2569");
        assert_eq!(thai_date(0), "1 ม.ค. 2513");
    }
}
