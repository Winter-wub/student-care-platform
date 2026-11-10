//! A roster file may leave out the columns that may be empty (#154): it
//! imports, the preview says the Students keep what is stored in them
//! (also in a `full` import), and a new Student starts with them empty.
//! A header close to an expected one names it, and a row's problem echoes
//! the value at fault, escaped and shortened.

mod roster_support;

use roster_support::*;

/// The required columns only: no email, Locale, Tags or GPA.
fn short_file(lines: &[&str]) -> Vec<u8> {
    let mut s = String::from("รหัสนักศึกษา,ชื่อ-สกุล,หลักสูตร,ชั้นปี\r\n");
    for l in lines {
        s.push_str(l);
        s.push_str("\r\n");
    }
    s.into_bytes()
}

#[tokio::test]
async fn a_full_import_without_the_optional_columns_keeps_the_stored_values() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    // A Student with an email, a Locale, roster Tags and a GPA.
    let first = csv_lines(&["68000001,นก ทะเลใส,nok@student.sealbay.example,en,ปี1;MSC,MSC,1,3.25"]);
    let id = previewed(&b, "a.csv", &first, "full", false).await;
    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");

    // The same Student a year on, and a new one, in a file with the
    // required columns only.
    let file = short_file(&["68000001,นก ทะเลใส,MSC,2", "68000002,ปลา ทะเลใส,MSC,1"]);
    let id = previewed(&b, "b.csv", &file, "full", false).await;
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    let at = page.find("id=\"roster-absent\"").expect("the absent card");
    let card = &page[at..at + page[at..].find("</section>").unwrap()];
    assert!(card.contains("data-absent=\"4\""), "{card}");
    for h in ["อีเมล", "ภาษา", "กลุ่ม", "เกรดเฉลี่ย"]
    {
        assert!(
            card.contains(&format!("<span class=\"name code-head\">{h}</span>")),
            "{h}: {card}"
        );
    }
    assert!(card.contains("คงค่าเดิม"), "{card}");
    // Only the year changes: nothing is cleared.
    let c = counts(&h, &id).await;
    assert_eq!(c["preview"]["updated"], 1, "{c}");
    assert_eq!(c["preview"]["new"], 1, "{c}");
    assert_eq!(c["preview"]["empty_email"], 0, "{c}");
    assert!(!page.contains("data-field=\"email\""), "{page}");
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );

    confirm(&b, &id).await;
    run_jobs(&h, &app).await;
    assert_eq!(import_text(&h, &id, "state").await, "done");
    let kept = "SELECT count(*) FROM student WHERE student_code = '68000001' \
                AND email = 'nok@student.sealbay.example' AND locale = 'en' \
                AND json_extract(attributes, '$.gpa') = 3.25 \
                AND json_extract(attributes, '$.year') = 2 AND status = 'active'";
    assert_eq!(int(&h, kept, &[]).await, 1);
    let tags = "SELECT count(*) FROM student_tag st JOIN student s ON s.id = st.student_id \
                WHERE s.student_code = '68000001' AND st.source = 'roster'";
    assert_eq!(int(&h, tags, &[]).await, 2, "ปี1; MSC kept");
    let new = "SELECT count(*) FROM student WHERE student_code = '68000002' \
               AND email IS NULL AND locale = 'th' \
               AND json_extract(attributes, '$.gpa') IS NULL";
    assert_eq!(int(&h, new, &[]).await, 1);
}

#[tokio::test]
async fn a_required_column_left_out_is_still_refused() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = "รหัสนักศึกษา,ชื่อ-สกุล,หลักสูตร\r\n68000001,นก,MSC\r\n";
    let r = upload(&b, "a.csv", file.as_bytes(), "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    let card = &page[page.find("id=\"expected-headers\"").unwrap()..];
    let mark = |state: &str, h: &str| {
        format!("data-header=\"{state}\"><span class=\"name code-head\">{h}</span>")
    };
    assert!(card.contains(&mark("missing", "ชั้นปี")), "{card}");
    assert!(card.contains(&mark("absent", "อีเมล")), "{card}");
    assert!(card.contains("ไม่มีในไฟล์ ไม่เป็นไร"), "{card}");
}

#[tokio::test]
async fn a_header_close_to_an_expected_one_names_it() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    // An English label, a space inside, and a typo.
    let file = "รหัส นักศึกษา,ชื่อ-สกุล,Email,หลักสูตร,ชั้นปี,เกรดเฉลี่ยย\r\n68000001,นก,,MSC,1,\r\n";
    let r = upload(&b, "a.csv", file.as_bytes(), "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    for (found, near) in [
        ("รหัส นักศึกษา", "รหัสนักศึกษา"),
        ("Email", "อีเมล"),
        ("เกรดเฉลี่ยย", "เกรดเฉลี่ย"),
    ] {
        let at = page
            .find(&format!("<span class=\"name code-head\">{found}</span>"))
            .unwrap_or_else(|| panic!("{found}: {page}"));
        let li = &page[at..at + page[at..].find("</li>").unwrap()];
        assert!(
            li.contains(&format!(
                "data-near=\"{near}\">ใกล้เคียงกับ <span class=\"code-head\">{near}</span>"
            )),
            "{li}"
        );
    }
    // The report says it too.
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(report.contains("(ใกล้เคียงกับ อีเมล)"), "{report}");
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_row_problem_echoes_the_value_escaped_and_shortened() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let long = "ก".repeat(60);
    let file = csv_lines(&[
        "6801OO05,นก,,,,MSC,1,",
        &format!("68000002,ปลา,,,,MSC,1,{long}"),
        "68000003,กุ้ง,,,,<b>MSC</b>,1,",
    ]);
    let r = upload(&b, "a.csv", &file, "full").await;
    let id = import_id(&r);
    assert_eq!(import_text(&h, &id, "state").await, "failed");
    let page = b.get(&format!("/admin/roster/{id}")).await.body;
    let value = |v: &str| format!("<span class=\"bad-value\">ค่าในไฟล์: <code>{v}</code></span>");
    assert!(page.contains(&value("6801OO05")), "{page}");
    let cut = format!("{}…", "ก".repeat(39));
    assert!(page.contains(&value(&cut)), "{page}");
    assert!(page.contains(&value("&lt;b&gt;MSC&lt;/b&gt;")), "{page}");
    assert!(!page.contains("<b>MSC</b>"), "{page}");
    let report = b.get(&format!("/admin/roster/{id}/report.csv")).await.body;
    assert!(
        report.lines().next().unwrap().ends_with(",ค่าในไฟล์"),
        "{report}"
    );
    assert!(report.contains(",6801OO05\r\n"), "{report}");
    assert_eq!(
        harpseal_testing::a11y::problems(&page),
        Vec::<String>::new()
    );
}
