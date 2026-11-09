//! The page of a refused roster file in plain Thai (#125): a problem with
//! the whole file says "ทั้งไฟล์", never "แถว 0", and a header problem sets
//! the headers found beside those the template expects, with the
//! template's link and a new upload (#134).

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
async fn a_header_mismatch_sets_the_found_headers_beside_the_expected_ones() {
    let file = "รหัสนักศึกษา,ชื่อ,อีเมล,ภาษา,กลุ่ม,หลักสูตร,ชั้นปี,เกรดเฉลี่ย\r\n68000001,นก,,,,MSC,1,\r\n";
    let page = page_of("bad.csv", file.as_bytes()).await;
    // The problem is stated once (#134): no problems table under it.
    assert_eq!(
        page.matches("หัวคอลัมน์ในไฟล์ไม่ตรงกับแม่แบบ").count(),
        1,
        "{page}"
    );
    assert!(!page.contains("<tr data-severity="), "{page}");
    let at = page
        .find("id=\"expected-headers\"")
        .expect("the expected headers");
    let card = &page[at..];
    let mark = |state: &str, h: &str| {
        format!("data-header=\"{state}\"><span class=\"name code-head\">{h}</span>")
    };
    // Found in the file: the wrong name is unknown, the others match.
    assert!(card.contains(&mark("unknown", "ชื่อ")), "{card}");
    assert!(card.contains(&mark("ok", "รหัสนักศึกษา")), "{card}");
    // Expected: required and may-be-empty apart, the missing one marked.
    assert!(card.contains("คอลัมน์ที่ต้องมีค่าทุกแถว"), "{card}");
    assert!(card.contains("คอลัมน์ที่เว้นว่างได้"), "{card}");
    assert!(card.contains(&mark("missing", "ชื่อ-สกุล")), "{card}");
    assert_eq!(card.matches("data-header=\"missing\"").count(), 1, "{card}");
    assert!(
        card.contains("href=\"/admin/roster/template.csv\""),
        "{card}"
    );
    // A new upload on the same page.
    assert!(page.contains("id=\"roster-reupload\""), "{page}");
    assert!(page.contains("action=\"/admin/roster/upload\""), "{page}");
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_file_without_any_header_row_says_which_headers_it_needs() {
    let page = page_of("none.csv", "a,b\r\n1,2\r\n".as_bytes()).await;
    let at = page
        .find("id=\"expected-headers\"")
        .expect("the expected headers");
    let card = &page[at..];
    // No row holds a column: nothing of the file is kept (its first row
    // may be a Student's data), and every column it needs is missing.
    assert!(card.contains("ไม่พบแถวหัวคอลัมน์ในไฟล์"), "{card}");
    assert!(
        !card.contains("<span class=\"name code-head\">a</span>"),
        "{card}"
    );
    assert_eq!(card.matches("data-header=\"missing\"").count(), 8, "{card}");
    for h in EXPECTED.split(", ") {
        assert!(
            card.contains(&format!("<span class=\"name code-head\">{h}</span>")),
            "{h}"
        );
    }
}

#[tokio::test]
async fn the_page_groups_row_numbers_and_the_csv_keeps_digits() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let mut file = csv(0..1100);
    file.extend_from_slice("6800000X,นก,,,,MSC,1,\r\n".as_bytes());
    let r = upload(&b, "big.csv", &file, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    assert!(
        page.contains("data-label=\"แถว\">1,102</td>"),
        "{}",
        first_row(&page)
    );
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("\n1102,รหัสนักศึกษา,"), "{report}");
}
