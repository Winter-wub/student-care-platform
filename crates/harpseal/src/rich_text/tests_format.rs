//! Headings and text colours (#113).

use super::*;
use maud::html as m;

fn t(s: &str) -> Inline {
    Inline::Text(s.into())
}

fn red(content: Vec<Inline>) -> Inline {
    Inline::Colour {
        colour: Colour::Red,
        content,
    }
}

fn line(s: &str) -> Vec<Inline> {
    match parse(s).blocks.as_slice() {
        [Block::Text(lines)] if lines.len() == 1 => lines[0].clone(),
        other => panic!("not one text line: {other:?}"),
    }
}

#[test]
fn headings_of_two_levels() {
    let doc = parse("## สอบปลายภาค\nวันจันทร์\n### ห้องสอบ\n- อาคาร 3");
    assert_eq!(
        doc.blocks,
        vec![
            Block::Heading {
                level: 2,
                content: vec![t("สอบปลายภาค")]
            },
            Block::Text(vec![vec![t("วันจันทร์")]]),
            Block::Heading {
                level: 3,
                content: vec![t("ห้องสอบ")]
            },
            Block::Bullets(vec![Item {
                marker: "- ".into(),
                number: None,
                content: vec![t("อาคาร 3")]
            }]),
        ]
    );
    // Chat: plain lines, the marker left out.
    assert_eq!(doc.plain(), "สอบปลายภาค\nวันจันทร์\nห้องสอบ\n- อาคาร 3");
    assert_eq!(
        doc.html().into_string(),
        "<div class=\"app-body\"><h2 class=\"app-h\">สอบปลายภาค</h2>วันจันทร์\
         <h3 class=\"app-h\">ห้องสอบ</h3><ul><li>อาคาร 3</li></ul></div>"
    );
    assert!(!doc.is_plain());
}

#[test]
fn a_heading_holds_inline_markup() {
    let url = "https://reg.seal-bay.invalid/exam";
    let doc = parse(&format!("## **ด่วน** [ตาราง]({url})"));
    assert_eq!(doc.plain(), "ด่วน ตาราง");
    assert_eq!(doc.first_links(2)[0].url, url);
}

#[test]
fn other_hashes_stay_text() {
    for s in [
        "#",
        "# หัวข้อ",
        "##",
        "## ",
        "##   ",
        "##หัวข้อ",
        "#### หัวข้อ",
        " ## หัวข้อ",
        "#1 ในใจ",
    ] {
        assert_eq!(line(s), vec![t(s)], "{s:?}");
        assert_eq!(plain(s), s, "{s:?}");
    }
}

#[test]
fn colours_of_the_palette() {
    assert_eq!(line("{red}ด่วน{/red}"), vec![red(vec![t("ด่วน")])]);
    for c in Colour::ALL {
        let s = format!("ก{}ข{}ค", c.open(), c.close());
        assert_eq!(
            line(&s),
            vec![
                t("ก"),
                Inline::Colour {
                    colour: c,
                    content: vec![t("ข")]
                },
                t("ค")
            ],
            "{s}"
        );
        assert_eq!(plain(&s), "กขค");
        let html = html(&s).into_string();
        assert!(
            html.contains(&format!("<span class=\"{}\">ข</span>", c.class())),
            "{html}"
        );
    }
}

#[test]
fn colour_nests_with_emphasis_but_not_in_itself() {
    assert_eq!(
        line("**{red}ก{/red}**"),
        vec![Inline::Bold(vec![red(vec![t("ก")])])]
    );
    assert_eq!(
        line("{red}**ก** *ข*{/red}"),
        vec![red(vec![
            Inline::Bold(vec![t("ก")]),
            t(" "),
            Inline::Italic(vec![t("ข")])
        ])]
    );
    // An inner colour stays text inside the outer one.
    assert_eq!(
        line("{red}ก{blue}ข{/blue}{/red}"),
        vec![red(vec![t("ก{blue}ข{/blue}")])]
    );
    // The first closer of its own name ends it.
    assert_eq!(
        line("{red}ก{/red}ข{/red}"),
        vec![red(vec![t("ก")]), t("ข{/red}")]
    );
    let url = "https://reg.seal-bay.invalid/exam";
    let s = format!("{{blue}}[ตาราง]({url}){{/blue}}");
    assert_eq!(parse(&s).links().len(), 1);
    assert_eq!(plain(&s), "ตาราง");
}

#[test]
fn hostile_colours_stay_text() {
    for s in [
        "{red}",
        "{/red}",
        "{red}{/red}",
        "{red}ก",
        "ก{/red}",
        "{pink}ก{/pink}",
        "{#ff0000}ก{/#ff0000}",
        "{Red}ก{/Red}",
        "{ red}ก{/red}",
        "{red}ก{/blue}",
        "{red}ก{ /red}",
        "{style=\"color:red\"}ก{/style}",
        "{}",
        "{{red}}",
    ] {
        // `{{red}}` holds `{red}` with no closer: text.
        assert_eq!(line(s), vec![t(s)], "{s:?}");
        assert_eq!(plain(s), s, "{s:?}");
    }
}

#[test]
fn colour_never_writes_markup_or_css() {
    let s = "{red}<script>alert(1)</script>\"style=x{/red}";
    let html = html(s).into_string();
    assert_eq!(
        html,
        "<p class=\"app-body\"><span class=\"tc-red\">&lt;script&gt;alert(1)&lt;/script&gt;\
         &quot;style=x</span></p>"
    );
    // A heading escapes as well.
    let html = html_of("## <b>x</b>");
    assert_eq!(
        html,
        "<div class=\"app-body\"><h2 class=\"app-h\">&lt;b&gt;x&lt;/b&gt;</h2></div>"
    );
}

fn html_of(s: &str) -> String {
    m! { (html(s)) }.into_string()
}

#[test]
fn a_plain_body_still_renders_as_before() {
    let body = "เรียนนักศึกษา\n\nสอบวันที่ 1 #ห้อง 3 {ไม่ใช่สี}\nขอบคุณ";
    assert_eq!(plain(body), body);
    assert!(parse(body).is_plain());
    assert_eq!(
        html(body).into_string(),
        format!("<p class=\"app-body\">{body}</p>")
    );
}

#[test]
fn palette_is_fixed_and_passes_contrast() {
    fn lum(hex: &str) -> f64 {
        let ch = |i: usize| {
            let c = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(1) + 0.7152 * ch(3) + 0.0722 * ch(5)
    }
    let ratio = |a: &str, b: &str| {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    let css = include_str!("../ui.css");
    for c in Colour::ALL {
        // Paper and Counter Mist: every surface a body sits on.
        for bg in ["#ffffff", "#f4f6f8"] {
            let r = ratio(c.hex(), bg);
            assert!(r >= 4.5, "{} on {bg}: {r:.2}", c.name());
        }
        let rule = format!(".{} {{ color: {}; }}", c.class(), c.hex());
        assert!(css.contains(&rule), "ui.css lacks {rule}");
    }
}
