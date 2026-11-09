//! The page of a refused roster file in plain Thai (#125): a problem with
//! the whole file says "ทั้งไฟล์", never "แถว 0", and a header problem
//! lists the headers the template expects, with the template's link.

mod roster_support;

use roster_support::*;

/// Uploads `bytes` and returns the refused import's page.
async fn page_of(name: &str, bytes: &[u8]) -> String {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let r = upload(&b, name, bytes, "full").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    b.get(&format!("/admin/roster/{id}")).await.body
}

/// The cells of the first row of the problems table.
fn first_row(page: &str) -> String {
    let at = page.find("<tr data-severity=").expect("a problems table");
    page[at..at + page[at..].find("</tr>").unwrap()].to_string()
}

const EXPECTED: &str = "รหัสนักศึกษา, ชื่อ-สกุล, อีเมล, ภาษา, กลุ่ม, หลักสูตร, ชั้นปี, เกรดเฉลี่ย";

#[tokio::test]
async fn a_file_level_problem_names_the_file_not_row_zero() {
    let page = page_of("roster.pdf", b"%PDF-1.7").await;
    let row = first_row(&page);
    assert!(row.contains("data-label=\"แถว\">ทั้งไฟล์</td>"), "{row}");
    assert!(!row.contains(">0</td>"), "{row}");
    // Not a header problem: no list of headers.
    assert!(!page.contains("id=\"expected-headers\""), "{page}");
}

#[tokio::test]
async fn a_header_mismatch_lists_the_expected_headers() {
    let file = "รหัสนักศึกษา,ชื่อ,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย\r\n68000001,นก,,,,MSC,1,\r\n";
    let page = page_of("bad.csv", file.as_bytes()).await;
    let at = page
        .find("id=\"expected-headers\"")
        .expect("the expected headers");
    let card = &page[at..];
    assert!(
        card.contains(&format!("<strong>{EXPECTED}</strong>")),
        "{card}"
    );
    assert!(
        card.contains("href=\"/admin/roster/template.csv\""),
        "{card}"
    );
    // The row of a header problem is the header row, as a number.
    assert!(
        first_row(&page).contains("data-label=\"แถว\">1</td>"),
        "{page}"
    );
}

#[tokio::test]
async fn a_file_without_any_header_row_says_which_headers_it_needs() {
    let page = page_of("none.csv", "a,b\r\n1,2\r\n".as_bytes()).await;
    let row = first_row(&page);
    assert!(row.contains("ทั้งไฟล์"), "{row}");
    assert!(row.contains(&format!("คอลัมน์ที่ต้องมี: {EXPECTED}")), "{row}");
    assert!(page.contains("id=\"expected-headers\""), "{page}");
}
