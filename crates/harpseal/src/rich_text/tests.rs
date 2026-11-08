use super::*;
use maud::html as m;

fn t(s: &str) -> Inline {
    Inline::Text(s.into())
}

fn link(text: &str, url: &str) -> Inline {
    Inline::Link {
        text: text.into(),
        url: url.into(),
    }
}

/// The pieces of a one-line body.
fn line(s: &str) -> Vec<Inline> {
    match parse(s).blocks.as_slice() {
        [Block::Text(lines)] if lines.len() == 1 => lines[0].clone(),
        other => panic!("not one text line: {other:?}"),
    }
}

const URL: &str = "https://reg.seal-bay.invalid/exam";

#[test]
fn emphasis() {
    assert_eq!(line("**ด่วน**"), vec![Inline::Bold(vec![t("ด่วน")])]);
    assert_eq!(
        line("ห้อง *3105* ครับ"),
        vec![t("ห้อง "), Inline::Italic(vec![t("3105")]), t(" ครับ")]
    );
    assert_eq!(
        line("**a *b* c**"),
        vec![Inline::Bold(vec![
            t("a "),
            Inline::Italic(vec![t("b")]),
            t(" c")
        ])]
    );
    assert_eq!(
        line("*a **b** c*"),
        vec![Inline::Italic(vec![
            t("a "),
            Inline::Bold(vec![t("b")]),
            t(" c")
        ])]
    );
    assert_eq!(
        line("***x***"),
        vec![Inline::Bold(vec![Inline::Italic(vec![t("x")])])]
    );
}

#[test]
fn unbalanced_and_spaced_markers_stay_text() {
    for s in [
        "**",
        "*",
        "** a**",
        "**a **",
        "* a*",
        "a * b * c",
        "**open",
        "close**",
        "2 * 3 = 6",
        "****",
        "***",
        "[",
        "[]()",
        "[x](",
        "[x]",
        "]x[",
    ] {
        assert_eq!(line(s), vec![t(s)], "{s}");
        assert_eq!(plain(s), s, "{s}");
    }
}

#[test]
fn nesting_is_bounded() {
    // Same kind never nests: the inner markers are text or close early.
    let deep = format!("{}x{}", "**".repeat(50), "**".repeat(50));
    let doc = parse(&deep);
    fn depth(p: &[Inline]) -> usize {
        p.iter()
            .map(|i| match i {
                Inline::Bold(c) | Inline::Italic(c) => 1 + depth(c),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }
    let Block::Text(lines) = &doc.blocks[0] else {
        panic!()
    };
    assert!(depth(&lines[0]) <= 2);
    let mixed = "*".repeat(3000);
    assert_eq!(plain(&mixed), mixed);
    // A link's text is plain.
    assert_eq!(line(&format!("[**x**]({URL})")), vec![link("**x**", URL)]);
    assert_eq!(
        line(&format!("**[ตาราง]({URL})**")),
        vec![Inline::Bold(vec![link("ตาราง", URL)])]
    );
}

#[test]
fn links_and_bare_urls() {
    assert_eq!(
        line(&format!("ดู[ตารางสอบ]({URL})ครับ")),
        vec![t("ดู"), link("ตารางสอบ", URL), t("ครับ")]
    );
    assert_eq!(
        line(&format!("ดูที่ {URL}.")),
        vec![t("ดูที่ "), link(URL, URL), t(".")]
    );
    assert_eq!(
        line(&format!("({URL})")),
        vec![t("("), link(URL, URL), t(")")]
    );
    let wiki = "https://seal-bay.invalid/a_(b)";
    assert_eq!(line(wiki), vec![link(wiki, wiki)]);
    assert_eq!(line(&format!("{URL}ครับ")), vec![link(URL, URL), t("ครับ")]);
    // Glued to a word: text.
    let glued = format!("x{URL}");
    assert_eq!(line(&glued), vec![t(&glued)]);
}

#[test]
fn hostile_links_stay_text() {
    for s in [
        "[x](javascript:alert(1))",
        "[x](JAVASCRIPT:alert(1))",
        "[x](data:text/html;base64,PHNjcmlwdD4=)",
        "[x](http://reg.seal-bay.invalid/)",
        "[x](//evil.invalid/)",
        "[x](https://user:pw@evil.invalid/)",
        "javascript:alert(1)",
        "http://seal-bay.invalid/",
        "https://user@evil.invalid/",
    ] {
        assert!(parse(s).links().is_empty(), "{s}");
        assert_eq!(plain(s), s, "{s}");
    }
    // The bracket form fails; the bare URL inside stops at the quote.
    let s = "[x](https://seal-bay.invalid/\"onclick=\"alert(1))";
    let links = parse(s).links();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://seal-bay.invalid/");
    assert_eq!(plain(s), s);
    let out = html(s).into_string();
    assert!(out.contains("&quot;onclick=&quot;alert(1))"), "{out}");
}

#[test]
fn html_is_escaped() {
    let s = "<script>alert(1)</script> & **<b>x</b>** [<i>](https://seal-bay.invalid/)";
    let out = html(s).into_string();
    assert!(!out.contains("<script>") && !out.contains("<b>") && !out.contains("<i>"));
    assert!(out.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; "));
    assert!(out.contains("<strong>&lt;b&gt;x&lt;/b&gt;</strong>"));
    assert!(out.contains(
        r#"<a href="https://seal-bay.invalid/" target="_blank" rel="noopener noreferrer" data-ext>&lt;i&gt;</a>"#
    ));
}

#[test]
fn lists() {
    let body = "ต้องเตรียม\n- บัตรนักศึกษา\n- ปากกา **สีน้ำเงิน**\n3. ข้อสาม\n4. ข้อสี่\nจบ";
    let doc = parse(body);
    assert_eq!(doc.blocks.len(), 4);
    assert!(matches!(&doc.blocks[1], Block::Bullets(i) if i.len() == 2));
    assert!(
        matches!(&doc.blocks[2], Block::Numbered(items) if items.len() == 2 && items[0].number == Some(3))
    );
    assert_eq!(
        doc.plain(),
        "ต้องเตรียม\n- บัตรนักศึกษา\n- ปากกา สีน้ำเงิน\n3. ข้อสาม\n4. ข้อสี่\nจบ"
    );
    assert_eq!(
        doc.html().into_string(),
        "<div class=\"app-body\">ต้องเตรียม<ul><li>บัตรนักศึกษา</li>\
         <li>ปากกา <strong>สีน้ำเงิน</strong></li></ul><ol><li value=\"3\">ข้อสาม</li>\
         <li value=\"4\">ข้อสี่</li></ol>จบ</div>"
    );
    // Not a marker: indented, no space, too many digits.
    for s in [
        " - x",
        "-x",
        "1.x",
        "1234567890. x",
        "ก. x",
        "0. x",
        "100. x",
        "2567. ภาคเรียนที่ 2",
    ] {
        assert!(
            matches!(parse(s).blocks.as_slice(), [Block::Text(_)]),
            "{s}"
        );
    }
}

#[test]
fn a_plain_body_renders_exactly_as_before() {
    let bodies = [
        "งานอาคารสถานที่จะปิดอาคารเรียนรวม 2 ทั้งวันเพื่อซ่อมระบบไฟฟ้า\r\n\r\nนักศึกษาที่มีเรียนชดเชยให้ไปที่ห้อง 3105",
        "Bring your ID card & a pen <blue>.\n\nRoom 3105 \"B\" wing, 2 * 3 rule",
        "",
    ];
    for body in bodies {
        let doc = parse(body);
        assert!(doc.is_plain(), "{body}");
        assert_eq!(doc.plain(), body);
        // The Mini App's markup before #108.
        let before = m! { p class="app-body" { (body) } }.into_string();
        assert_eq!(doc.html().into_string(), before);
    }
}

#[test]
fn first_links_are_distinct_and_in_order() {
    let a = "https://a.seal-bay.invalid/";
    let b = "https://b.seal-bay.invalid/";
    let c = "https://c.seal-bay.invalid/";
    let body = format!("[ก]({a}) {a}\n- [ข]({b})\n**[ค]({c})**");
    let got = parse(&body).first_links(2);
    assert_eq!(
        got,
        vec![
            Link {
                text: "ก".into(),
                url: a.into()
            },
            Link {
                text: "ข".into(),
                url: b.into()
            },
        ]
    );
    assert_eq!(parse(&body).links().len(), 4);
}

#[test]
fn long_lines_parse_in_bounds() {
    let long = format!("{} {URL} ", "ก".repeat(20_000));
    let doc = parse(&long);
    assert_eq!(doc.links().len(), 1);
    let stars = "*a ".repeat(5_000);
    assert_eq!(plain(&stars), stars);
    let opens = "**a ".repeat(5_000);
    assert_eq!(plain(&opens), opens);
    let brackets = "[x](".repeat(5_000);
    assert_eq!(plain(&brackets), brackets);
}

#[test]
fn bracket_urls_balance_parentheses() {
    let wiki = "https://seal-bay.invalid/a_(b)";
    assert_eq!(line(&format!("[วิกิ]({wiki})")), vec![link("วิกิ", wiki)]);
    let open = "[x](https://seal-bay.invalid/a_(b)";
    assert_eq!(parse(open).links().len(), 1, "only the bare URL inside");
    assert_eq!(plain(open), open);
}

/// Each hostile line at `n` repetitions and at 8 times that: a quadratic
/// parse would take 64 times as long; a linear one about 8 times. The
/// bound is generous (debug builds, a busy CI runner), the ratio is not.
#[test]
fn hostile_lines_parse_in_linear_time() {
    use std::time::{Duration, Instant};
    let patterns = [
        "**a ",
        "*a ",
        "[",
        "[x](",
        "[x](a(",
        "*",
        "**",
        "*** ",
        "https://a,",
        "https://seal-bay.invalid/)",
        "]([",
    ];
    let time = |s: &str| {
        let t = Instant::now();
        let doc = parse(s);
        assert!(doc.plain().len() <= s.len());
        t.elapsed()
    };
    for p in patterns {
        let n = 2_000 / p.len() + 1;
        let small = p.repeat(n * 10);
        let big = p.repeat(n * 80);
        let (a, b) = (time(&small), time(&big));
        assert!(b < Duration::from_secs(2), "{p:?}: {b:?}");
        // 8x the input may take at most 32x the time (64x is quadratic).
        assert!(
            b <= a * 32 + Duration::from_millis(20),
            "{p:?}: {a:?} -> {b:?}"
        );
    }
    // The 2,000-character body itself.
    let body = "**a [".repeat(400);
    assert!(time(&body) < Duration::from_millis(200));
}
