//! Markup checks a back-office page must pass (#129), cheap enough to run
//! on every page a test walks: heading order, labelled fields and live
//! regions. Not a full HTML parser: it reads the tags maud writes.
//!
//! ```
//! let page = r#"<main><h1>นักศึกษา</h1><label for="q">ค้นหา</label><input id="q"></main>"#;
//! assert!(harpseal_testing::a11y::problems(page).is_empty());
//! let bad = r#"<h1>a</h1><h3>b</h3><input name="year">"#;
//! assert_eq!(harpseal_testing::a11y::problems(bad).len(), 2);
//! ```

/// One start or end tag.
#[derive(Debug)]
struct Tag<'a> {
    name: String,
    end: bool,
    attrs: &'a str,
    /// Byte offset just after the tag.
    after: usize,
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// The tags of `html`, skipping comments, scripts and styles.
fn tags(html: &str) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(lt) = html[i..].find('<').map(|o| i + o) {
        if html[lt..].starts_with("<!--") {
            i = html[lt..].find("-->").map_or(html.len(), |e| lt + e + 3);
            continue;
        }
        let Some(gt) = html[lt..].find('>').map(|o| lt + o) else {
            break;
        };
        let inner = &html[lt + 1..gt];
        let end = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let name_len = inner
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(inner.len());
        let name = inner[..name_len].to_ascii_lowercase();
        i = gt + 1;
        if name.is_empty() || name.starts_with('!') {
            continue;
        }
        if !end && (name == "script" || name == "style") {
            let close = format!("</{name}");
            i = html[i..].find(&close).map_or(html.len(), |c| i + c);
        }
        out.push(Tag {
            name,
            end,
            attrs: &inner[name_len..],
            after: i,
        });
    }
    out
}

/// The value of attribute `name` in `attrs`, if present (`""` for a bare one).
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attrs;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let tail = &rest[at + name.len()..];
        rest = tail;
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        if let Some(v) = tail.strip_prefix("=\"") {
            return v.split('"').next();
        }
        if tail.is_empty() || tail.starts_with(char::is_whitespace) || tail.starts_with('/') {
            return Some("");
        }
    }
    None
}

/// The text between the tag ending at `from` and its closing tag `name`.
fn text_until(html: &str, from: usize, name: &str) -> String {
    let close = format!("</{name}>");
    let end = html[from..].find(&close).map_or(html.len(), |e| from + e);
    let mut s = String::new();
    let mut inside = false;
    for c in html[from..end].chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => s.push(c),
            _ => {}
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_live(attrs: &str) -> bool {
    matches!(attr(attrs, "role"), Some("status" | "alert"))
        || attr(attrs, "aria-live").is_some_and(|v| v != "off")
}

/// What is wrong with `html` (a whole page or a fragment), one line each:
///
/// - **Headings:** exactly one `h1` (in a whole page) and no skipped level
///   going down (`h2` then `h4`).
/// - **Fields:** every `input` (but hidden, submit, button, reset and
///   image), `select` and `textarea` has a `label for` its id, sits inside
///   a `label`, or has `aria-label` / `aria-labelledby`.
/// - **Live regions:** none inside another (it would be announced twice),
///   and at most one `role="status"` / `role="alert"` with text inside
///   `main` when the page loads (the flash or the page's own message, not
///   both).
pub fn problems(html: &str) -> Vec<String> {
    let tags = tags(html);
    let mut out = Vec::new();

    // Headings.
    let mut last = 0u8;
    let mut h1 = 0;
    for t in tags.iter().filter(|t| !t.end) {
        let b = t.name.as_bytes();
        if b.len() == 2 && b[0] == b'h' && (b'1'..=b'6').contains(&b[1]) {
            let level = b[1] - b'0';
            if level == 1 {
                h1 += 1;
            }
            if last > 0 && level > last + 1 {
                let text = text_until(html, t.after, &t.name);
                out.push(format!("heading h{last} then h{level}: {text}"));
            }
            last = level;
        }
    }
    if html.contains("<html") && h1 != 1 {
        out.push(format!("{h1} h1 on the page"));
    }

    // Fields.
    let label_for: Vec<&str> = tags
        .iter()
        .filter(|t| !t.end && t.name == "label")
        .filter_map(|t| attr(t.attrs, "for"))
        .collect();
    let mut in_label = 0usize;
    for t in &tags {
        match (t.name.as_str(), t.end) {
            ("label", false) => in_label += 1,
            ("label", true) => in_label = in_label.saturating_sub(1),
            ("input" | "select" | "textarea", false) => {
                let ty = attr(t.attrs, "type").unwrap_or("text");
                if t.name == "input"
                    && matches!(ty, "hidden" | "submit" | "button" | "reset" | "image")
                {
                    continue;
                }
                let named = ["aria-label", "aria-labelledby"]
                    .iter()
                    .any(|a| attr(t.attrs, a).is_some_and(|v| !v.trim().is_empty()));
                let tied = attr(t.attrs, "id").is_some_and(|id| label_for.contains(&id));
                if !(named || tied || in_label > 0) {
                    let name = attr(t.attrs, "name").unwrap_or("?");
                    out.push(format!("unlabelled {} name={name}", t.name));
                }
            }
            _ => {}
        }
    }

    // Live regions: a stack of open elements, each marked live or not.
    let mut stack: Vec<(String, bool)> = Vec::new();
    let mut in_main = false;
    let mut speaking = Vec::new();
    for t in &tags {
        if VOID.contains(&t.name.as_str()) {
            continue;
        }
        if t.end {
            if let Some(at) = stack.iter().rposition(|(n, _)| *n == t.name) {
                stack.truncate(at);
            }
            if t.name == "main" {
                in_main = false;
            }
            continue;
        }
        if t.name == "main" {
            in_main = true;
        }
        let live = is_live(t.attrs);
        if live && stack.iter().any(|(_, l)| *l) {
            let text = text_until(html, t.after, &t.name);
            out.push(format!("live region inside a live region: {text}"));
        }
        let role = attr(t.attrs, "role");
        if live && in_main && matches!(role, Some("status" | "alert")) {
            let text = text_until(html, t.after, &t.name);
            if !text.is_empty() {
                speaking.push(text);
            }
        }
        stack.push((t.name.clone(), live));
    }
    if speaking.len() > 1 {
        out.push(format!(
            "{} messages announced on load: {}",
            speaking.len(),
            speaking.join(" | ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::problems;

    #[test]
    fn headings_go_down_one_level_at_a_time() {
        assert!(problems("<h1>a</h1><h2>b</h2><h3>c</h3><h2>d</h2>").is_empty());
        assert_eq!(problems("<h1>a</h1><h4>b</h4>").len(), 1);
        let page = "<html><body><h1>a</h1><h1>b</h1></body></html>";
        assert_eq!(problems(page), ["2 h1 on the page"]);
    }

    #[test]
    fn fields_need_a_label() {
        let ok = r#"<label for="a">A</label><input id="a"><label>B <select name="b"></select></label>
            <input type="hidden" name="_csrf"><textarea aria-label="C"></textarea>"#;
        assert!(problems(ok).is_empty(), "{:?}", problems(ok));
        assert_eq!(
            problems(r#"<input name="year_min" id="y">"#),
            ["unlabelled input name=year_min"]
        );
    }

    #[test]
    fn one_message_on_load_and_no_nested_live_regions() {
        let twice = r#"<main><div role="status">ไม่ได้นำเข้า</div><div class="warn" role="alert">ไฟล์ถูกปฏิเสธ</div></main>"#;
        assert_eq!(problems(twice).len(), 1);
        let nested = r#"<div aria-live="polite"><p role="alert">x</p></div>"#;
        assert_eq!(problems(nested).len(), 1);
        let empty = r#"<main><div role="status"></div><p role="alert">x</p></main>"#;
        assert!(problems(empty).is_empty());
    }
}
