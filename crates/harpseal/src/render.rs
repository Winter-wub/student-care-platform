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
    pub fn mini_app_url(&self, path: &str) -> Option<String> {
        self.mini_app_base_url
            .as_ref()
            .map(|base| format!("{base}{path}"))
    }

    pub fn is_thai(&self) -> bool {
        self.locale.0 == "th"
    }
}

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

/// Every Action id a Message carries, in order. Renderers must emit each
/// of them exactly once (tests check this: no Action is dropped silently).
pub fn action_ids(msg: &Message) -> Vec<String> {
    let buttons: Vec<&Button> = match msg {
        Message::Card(c) => c.buttons.iter().collect(),
        Message::CardList(l) => l.0.iter().flat_map(|c| c.buttons.iter()).collect(),
        Message::Choices(c) => c.options.iter().collect(),
        _ => Vec::new(),
    };
    buttons
        .into_iter()
        .filter_map(|b| match b {
            Button::Action { action, .. } => Some(action.as_str().to_string()),
            Button::MiniApp { .. } => None,
        })
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

pub fn file_link(cx: &RenderCx, name: &str, size_bytes: u64, url: &str) -> String {
    let mb = size_bytes as f64 / 1_048_576.0;
    if cx.is_thai() {
        format!("ไฟล์แนบ: {name} ({mb:.1} MB)\nดาวน์โหลด: {url}")
    } else {
        format!("Attachment: {name} ({mb:.1} MB)\nDownload: {url}")
    }
}

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
    fn truncate_marks_the_cut() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }
}
