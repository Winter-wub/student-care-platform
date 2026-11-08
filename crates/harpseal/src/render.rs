//! What every Channel adapter's renderer shares (issue #3): the render
//! context and deterministic text helpers. Lengths are counted in Unicode
//! scalar values (`char`s); Thai is in the BMP, so this equals UTF-16 units.

use crate::message::{Button, Locale, Message};

/// What a renderer needs besides the Message. Built per send by the adapter.
#[derive(Debug, Clone)]
pub struct RenderCx {
    /// The Student's Locale, for the adapter's own degrade texts.
    pub locale: Locale,
    /// Base URL a `Button::MiniApp` path is appended to (LIFF URL on LINE,
    /// the Web App URL on Telegram). `None`: no Mini App configured.
    pub mini_app_base_url: Option<String>,
}

impl RenderCx {
    /// The full Mini App URL for `path`, if a Mini App is configured.
    pub fn mini_app_url(&self, path: &str) -> Option<String> {
        self.mini_app_base_url
            .as_ref()
            .map(|base| format!("{base}{path}"))
    }

    /// Whether degrade texts should be in Thai.
    pub fn is_thai(&self) -> bool {
        self.locale.0 == "th"
    }
}

/// Length in Unicode scalar values.
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// At most `max` chars; a cut string ends with `…`.
pub fn truncate(s: &str, max: usize) -> String {
    if char_len(s) <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Splits into chunks of at most `max` chars. Breaks after the last newline
/// in the second half of a chunk, else after the last space there, else
/// exactly at `max`. Never returns an empty Vec for non-empty input.
pub fn split_text(s: &str, max: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while chars.len() - start > max {
        let window = &chars[start..start + max];
        let half = max / 2;
        let cut = window[half..]
            .iter()
            .rposition(|c| *c == '\n')
            .or_else(|| window[half..].iter().rposition(|c| *c == ' '))
            .map(|i| half + i + 1)
            .unwrap_or(max);
        out.push(
            chars[start..start + cut]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_string(),
        );
        start += cut;
    }
    out.push(chars[start..].iter().collect());
    out
}

/// Longest URL in bytes a `Button::Url` or a body link carries: LINE's
/// URI action limit (<https://developers.line.biz/en/reference/messaging-api/#uri-action>),
/// the stricter of the two Channels.
pub const URL_MAX: usize = 1000;

/// `https://host[:port][/…]`: a host of letters, digits, `.` and `-`, no
/// user info, no spaces. The check `[settings]` URLs have always had.
pub fn https_url(v: &str) -> bool {
    let Some(rest) = v.strip_prefix("https://") else {
        return false;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && !host.starts_with(['.', '-'])
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    host_ok && port_ok && !tail.chars().any(char::is_whitespace)
}

/// A URL a Student may be sent to (a `Button::Url`, a link in a body),
/// one LINE and Telegram both accept: [`https_url`], at most [`URL_MAX`]
/// bytes, only printable ASCII without `"`, `'`, `<`, `>`, `\`, `` ` ``,
/// `^`, `{`, `|`, `}`, `[`, `]`; every `%` followed by two hex digits; at
/// most one `#`; a host of two or more non-empty labels, none starting or
/// ending with `-`. A pasted URL is percent-encoded; anything else stays
/// plain text (no button), so a Push is never refused for a URL.
pub fn link_url(v: &str) -> bool {
    v.len() <= URL_MAX
        && https_url(v)
        && v.bytes().all(|b| {
            b.is_ascii_graphic()
                && !matches!(
                    b,
                    b'"' | b'\''
                        | b'<'
                        | b'>'
                        | b'\\'
                        | b'`'
                        | b'^'
                        | b'{'
                        | b'|'
                        | b'}'
                        | b'['
                        | b']'
                )
        })
        && v.matches('#').count() <= 1
        && percent_escapes_ok(v)
        && host_labels_ok(v)
}

/// Every `%` starts a `%XX` escape.
fn percent_escapes_ok(v: &str) -> bool {
    let b = v.as_bytes();
    b.iter().enumerate().all(|(i, &c)| {
        c != b'%'
            || (b.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit))
    })
}

/// Two or more labels, none empty, none starting or ending with `-`.
fn host_labels_ok(v: &str) -> bool {
    let rest = v.strip_prefix("https://").unwrap_or(v);
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let host = authority.split(':').next().unwrap_or_default();
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-'))
}

/// Whether `c` is a Unicode format character (general category Cf): the
/// soft hyphen, Arabic number signs, zero-width and joiner characters,
/// bidi marks, embeddings, overrides and isolates, invisible operators,
/// the BOM, interlinear annotation marks, tag characters.
pub fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// `s` without format characters ([`is_format_char`]), for a button label.
pub fn strip_format_chars(s: &str) -> String {
    s.chars().filter(|&c| !is_format_char(c)).collect()
}

/// Every Action id a Message carries, in order. Renderers must emit each
/// of them exactly once (tests check this: no Action is dropped silently).
pub fn action_ids(msg: &Message) -> Vec<String> {
    msg.buttons()
        .into_iter()
        .filter_map(Button::action_id)
        .map(|a| a.as_str().to_string())
        .collect()
}

/// The adapter's own degrade texts, in Thai or English.
pub fn mini_app_unavailable(cx: &RenderCx, label: &str) -> String {
    if cx.is_thai() {
        format!("(“{label}” ยังเปิดในช่องทางนี้ไม่ได้ ติดต่อเจ้าหน้าที่หากต้องการข้อมูลนี้)")
    } else {
        format!("(“{label}” cannot be opened on this channel yet; ask Staff for it.)")
    }
}

/// The line an adapter shows instead of a URL button it refuses (a URL
/// that is not https; only code inside the Core can build one).
pub fn link_unavailable(cx: &RenderCx, label: &str) -> String {
    if cx.is_thai() {
        format!("(ลิงก์ “{label}” เปิดไม่ได้)")
    } else {
        format!("(The link “{label}” cannot be opened.)")
    }
}

/// The text an adapter sends instead of a file it cannot attach.
pub fn file_link(cx: &RenderCx, name: &str, size_bytes: u64, url: &str) -> String {
    let mb = size_bytes as f64 / 1_048_576.0;
    if cx.is_thai() {
        format!("ไฟล์แนบ: {name} ({mb:.1} MB)\nดาวน์โหลด: {url}")
    } else {
        format!("Attachment: {name} ({mb:.1} MB)\nDownload: {url}")
    }
}

/// Label of a follow-up message carrying overflow buttons.
pub fn more_label(cx: &RenderCx, part: usize, of: usize) -> String {
    if cx.is_thai() {
        format!("ตัวเลือกเพิ่มเติม ({part}/{of})")
    } else {
        format!("More options ({part}/{of})")
    }
}

/// Reserved Kernel Action: a Menu with more entries than the channel can
/// show gets one last entry with this id; the Kernel answers it with the
/// remaining entries as Choices.
pub const MENU_MORE_ACTION: &str = "kernel.menu.more";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_prefers_newlines_and_respects_max() {
        let s = format!("{}\n{}", "ก".repeat(70), "ข".repeat(70));
        let parts = split_text(&s, 100);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "ก".repeat(70));
        assert!(parts.iter().all(|p| char_len(p) <= 100));
        assert_eq!(split_text("abc", 100), vec!["abc".to_string()]);
    }

    #[test]
    fn link_urls_are_https_only() {
        assert!(link_url("https://reg.seal-bay.invalid/exam?term=2#room"));
        assert!(link_url("https://seal-bay.invalid:8443/a%E0%B8%81"));
        for bad in [
            "http://seal-bay.invalid/",
            "HTTPS://seal-bay.invalid/",
            "javascript:alert(1)",
            "data:text/html,x",
            "https://user@seal-bay.invalid/",
            "https://seal-bay.invalid\\@evil.invalid/",
            "https://seal-bay.invalid/\"onmouseover=x",
            "https://seal-bay.invalid/<script>",
            "https://seal-bay.invalid/ก",
            "https://seal-bay.invalid/a b",
            "https:///path",
            "https://",
            "https://localhost/",
            "https://seal-bay./",
            "https://a..invalid/",
            "https://-seal.invalid/",
            "https://seal-.invalid/",
            "https://seal-bay.invalid/a%zz",
            "https://seal-bay.invalid/a%4",
            "https://seal-bay.invalid/a#b#c",
            "https://seal-bay.invalid/[x]",
        ] {
            assert!(!link_url(bad), "{bad}");
        }
        let long = format!("https://seal-bay.invalid/{}", "a".repeat(URL_MAX));
        assert!(!link_url(&long));
    }

    #[test]
    fn format_chars_leave_labels() {
        let l = "\u{202E}ตาราง\u{200B}สอบ\u{2066}x\u{FEFF}";
        assert_eq!(strip_format_chars(l), "ตารางสอบx");
        assert_eq!(strip_format_chars("ปกติ"), "ปกติ");
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }
}
