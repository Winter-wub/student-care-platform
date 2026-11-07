//! The Student's side of Fees (#75): a Mini App page, linked from the
//! Mini App home, with the Student's own outstanding balance and the
//! bank account to pay into, the Module setting
//! [`super::BANK_ACCOUNT`] read fresh on every view, so a change on
//! `/admin/settings` shows on the next open.
//!
//! Like "My info", the handler takes [`AppStudent`] and nothing else from
//! the request: no URL or form value can show another Student's balance.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use harpseal::{AppState, AppStudent, CatalogEntry, Statement};
use maud::html;

use super::read::baht;

/// The page, under `/app/` (App Session cookie, Mini App CSP).
pub const PATH: &str = "/app/fees";

/// The page's texts and the setting's label and help.
pub(super) const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("fees.mine", "th", "ค่าธรรมเนียมของฉัน"),
    CatalogEntry::new("fees.mine", "en", "My fees"),
    CatalogEntry::new("fees.balance", "th", "ยอดค้างชำระ"),
    CatalogEntry::new("fees.balance", "en", "Outstanding"),
    CatalogEntry::new("fees.nothing", "th", "ไม่มียอดค้างชำระ"),
    CatalogEntry::new("fees.nothing", "en", "Nothing outstanding"),
    CatalogEntry::new("fees.pay_to", "th", "ชำระเข้าบัญชี"),
    CatalogEntry::new("fees.pay_to", "en", "Pay into"),
    CatalogEntry::new("fees.bank_account", "th", "บัญชีรับชำระค่าธรรมเนียม"),
    CatalogEntry::new("fees.bank_account", "en", "Bank account for fees"),
    CatalogEntry::new(
        "fees.bank_account_help",
        "th",
        "แสดงให้นักศึกษาเห็นในหน้า ค่าธรรมเนียมของฉัน ทันทีที่บันทึก",
    ),
    CatalogEntry::new(
        "fees.bank_account_help",
        "en",
        "Shown to Students on My fees as soon as it is saved.",
    ),
    CatalogEntry::new("fees.back", "th", "กลับหน้าของฉัน"),
    CatalogEntry::new("fees.back", "en", "Back to my page"),
];

pub(super) async fn page(State(s): State<AppState>, me: AppStudent) -> Response {
    let q = Statement::new("SELECT amount_satang FROM fees_balance WHERE student_code = ?")
        .bind(me.student_code.as_str());
    let owed = match s.services.db.query(q).await {
        Ok(rows) => rows.get(0, "amount_satang").and_then(|v| v.as_i64()),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Ok(account) = s.settings().text(super::BANK_ACCOUNT).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let title = me.t("fees.mine");
    let body = html! {
        section class="card" {
            h1 { (title) }
            dl {
                dt { (me.t("fees.balance")) }
                dd id="balance" {
                    @match owed {
                        Some(n) if n > 0 => { (baht(n)) " บาท" }
                        _ => { (me.t("fees.nothing")) }
                    }
                }
            }
        }
        section class="card" id="bank-account" {
            h2 { (me.t("fees.pay_to")) }
            p { @for (i, line) in account.lines().enumerate() {
                @if i > 0 { br; } (line)
            } }
        }
        p { a class="btn secondary" href="/app" { (me.t("fees.back")) } }
    };
    me.page(&s, &title, body)
}
