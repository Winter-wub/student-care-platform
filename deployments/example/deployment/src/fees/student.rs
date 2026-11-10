//! The Student's side of Fees (#75): a Mini App page, linked from the
//! Mini App home, with the Student's own outstanding balance and the
//! bank account to pay into, the Module settings [`super::BANK_NAME`] and
//! [`super::BANK_ACCOUNT`] read fresh on every view, so a change on
//! `/admin/settings` shows on the next open. A saved value that fails its
//! check, or one nobody has saved yet ([`super::BANK_ACCOUNT`] has no
//! default), shows "not set", never a placeholder account.
//!
//! Like "My info", the handler takes [`AppStudent`] and nothing else from
//! the request: no URL or form value can show another Student's balance.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use harpseal::web::axum;
use harpseal::{AppState, AppStudent, CatalogEntry, Error, ErrorReport, Statement};
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
    CatalogEntry::new("fees.not_set", "th", "ยังไม่ได้ตั้งค่า ติดต่อกองคลัง"),
    CatalogEntry::new("fees.not_set", "en", "Not set yet; ask the finance office"),
    CatalogEntry::new("fees.bank_name_label", "th", "ธนาคารและชื่อบัญชีรับชำระ"),
    CatalogEntry::new(
        "fees.bank_name_label",
        "en",
        "Bank and account name for fees",
    ),
    CatalogEntry::new(
        "fees.bank_name_help",
        "th",
        "แสดงให้นักศึกษาเห็นในหน้า ค่าธรรมเนียมของฉัน ทันทีที่บันทึก",
    ),
    CatalogEntry::new(
        "fees.bank_name_help",
        "en",
        "Shown to Students on My fees as soon as it is saved.",
    ),
    CatalogEntry::new("fees.bank_account_label", "th", "เลขที่บัญชีรับชำระ"),
    CatalogEntry::new("fees.bank_account_label", "en", "Account number for fees"),
    CatalogEntry::new(
        "fees.bank_account_help",
        "th",
        "เฉพาะตัวเลข ขีด และช่องว่าง นักศึกษาคัดลอกไปโอนได้ตรงตัว",
    ),
    CatalogEntry::new(
        "fees.bank_account_help",
        "en",
        "Digits, hyphens and spaces only, so Students copy it exactly.",
    ),
    CatalogEntry::new("fees.back", "th", "กลับหน้าของฉัน"),
    CatalogEntry::new("fees.back", "en", "Back to my page"),
];

/// The 500 of the page, with the [`ErrorReport`] the runtime logs (#162):
/// a Mini App page has no back-office shell, so it attaches the report
/// itself.
fn failed(e: &Error) -> Response {
    let page = StatusCode::INTERNAL_SERVER_ERROR.into_response();
    ErrorReport::attach(page, "fees.student", e)
}

pub(super) async fn page(State(s): State<AppState>, me: AppStudent) -> Response {
    let q = Statement::new("SELECT amount_satang FROM fees_balance WHERE student_code = ?")
        .bind(me.student_code.as_str());
    let owed = match s.services.db.query(q).await {
        Ok(rows) => rows.get(0, "amount_satang").and_then(|v| v.as_i64()),
        Err(e) => return failed(&e),
    };
    // A saved value failing its check reads as "not set"; any other
    // error (the database) is a failure, never "not set".
    let settings = s.settings();
    let (bank, account) = match (
        not_set_is_none(settings.text(super::BANK_NAME).await),
        not_set_is_none(settings.text(super::BANK_ACCOUNT).await),
    ) {
        (Ok(bank), Ok(account)) => (bank, account),
        (Err(e), _) | (_, Err(e)) => return failed(&e),
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
            @match (bank, account) {
                (Some(bank), Some(account)) => {
                    p id="bank-name" { (bank) }
                    p { strong id="account-number" class="code" { (account) } }
                }
                _ => { p id="not-set" { (me.t("fees.not_set")) } }
            }
        }
        p { a class="btn secondary" href="/app" { (me.t("fees.back")) } }
    };
    me.page(&s, &title, body)
}

/// `Ok(None)` for [`Error::SettingNotSet`]; every other error stays one.
fn not_set_is_none(r: harpseal::Result<String>) -> harpseal::Result<Option<String>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(Error::SettingNotSet(_)) => Ok(None),
        Err(e) => Err(e),
    }
}
