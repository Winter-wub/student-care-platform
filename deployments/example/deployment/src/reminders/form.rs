//! The list form of "ตรวจก่อนส่งเตือน" (#146): the Core's Student picker
//! ([`ui::StudentPicker`], a textarea of codes without JS) under the
//! field `ids`, "ตรวจก่อนส่งเตือน" and "ร่างประกาศ…"; and "เริ่มจากรายการ
//! ค้างชำระ" (`?from=fees`), which fills the list with the active Students
//! who owe a fee (`fees_balance`), the first [`MAX_NOTIFY`] by code.

use harpseal::{AppState, CurrentStaff, MAX_NOTIFY, Result, Statement, Value, ui};
use maud::{Markup, html};

use super::{ANNOUNCE_PATH, PATH, PICK};

/// The query that starts from the fee list.
pub(super) const FROM_FEES: &str = "from=fees";

/// The form: the picked `ids` under `key`, and the two actions. Also on
/// the review step when the list is too long to go back by a link (#135).
pub(super) async fn list_form(
    s: &AppState,
    staff: &CurrentStaff,
    key: &str,
    ids: Vec<String>,
) -> Result<Markup> {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let picker = ui::StudentPicker::new("ids", PICK, &t("reminders.ids"))
        .max(MAX_NOTIFY)
        .codes(ids)
        .render(s, staff)
        .await?;
    Ok(html! {
        form method="post" action={ "/admin" (PATH) } {
            (staff.csrf_field())
            input type="hidden" name="key" value=(key);
            (picker)
            // An action row: the two buttons never touch (#131).
            div class="actions" {
                button type="submit" { (t("reminders.send")) }
                button type="submit" class="secondary" formaction={ "/admin" (ANNOUNCE_PATH) } {
                    (t("reminders.announce"))
                }
            }
        }
    })
}

/// "เริ่มจากรายการค้างชำระ": a link, so it works without JS too.
pub(super) fn from_fees_link(s: &AppState, staff: &CurrentStaff) -> Markup {
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    html! {
        div class="actions picker-start" {
            a class="btn secondary" href={ "/admin" (PATH) "?" (FROM_FEES) } { (t("reminders.from_fees")) }
        }
    }
}

/// The active Students who owe a fee: the first [`MAX_NOTIFY`] codes in
/// code order, and how many owe in all.
pub(super) async fn owing(s: &AppState) -> Result<(Vec<String>, usize)> {
    let from = "FROM fees_balance f JOIN student s ON s.student_code = f.student_code \
                WHERE f.amount_satang > 0 AND s.status = 'active'";
    let count = Statement::new(format!("SELECT count(*) AS n {from}"));
    let rows = s.services.db.query(count).await?;
    let total = rows.get(0, "n").and_then(Value::as_i64).unwrap_or(0);
    let list = Statement::new(format!(
        "SELECT f.student_code {from} ORDER BY f.student_code LIMIT ?"
    ))
    .bind(MAX_NOTIFY as i64);
    let rows = s.services.db.query(list).await?;
    let codes = (0..rows.len())
        .filter_map(|i| rows.get(i, "student_code").and_then(Value::as_text))
        .map(str::to_string)
        .collect();
    Ok((codes, usize::try_from(total).unwrap_or(0)))
}

/// The note above the form after "เริ่มจากรายการค้างชำระ": how many were
/// picked, and why only the first ones when more owe.
pub(super) fn from_fees_note(
    s: &AppState,
    staff: &CurrentStaff,
    picked: usize,
    total: usize,
) -> Markup {
    let n = ui::number(picked as i64);
    let all = ui::number(total as i64);
    let args = [("n", n.as_str()), ("total", all.as_str())];
    let key = match total {
        0 => "reminders.from_fees_none",
        t if t > picked => "reminders.from_fees_capped",
        _ => "reminders.from_fees_done",
    };
    html! { p class="note" { (s.catalog.tf(&staff.locale, key, &args)) } }
}
