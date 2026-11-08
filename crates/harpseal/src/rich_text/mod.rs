//! The body format of an Announcement (#108): a small Markdown subset
//! stored as plain text. Parsed here, in the Core, with no new crate.
//!
//! Grammar, line by line (a construct never spans lines):
//!
//! - A line starting `- ` is a bullet item; a line starting with 1 to 9
//!   digits, `.` and a space is a numbered item. Consecutive items of one
//!   kind make one list (a numbered list starts at its first number).
//!   Lists do not nest: an indented marker is text.
//! - Any other line is text; line breaks and blank lines are kept as
//!   typed (the Mini App shows them with `white-space: pre-line`).
//! - Inside a line: `**bold**`, `*italic*` (the opening marker followed
//!   and the closing one preceded by a non-space), `[text](https://…)`,
//!   and a bare `https://…` (ending at a space; trailing `.,;:!?` and an
//!   unbalanced `)` are left out). Bold may hold italic and the reverse,
//!   either may hold a link; a link's text is plain. That bounds nesting
//!   at three levels whatever the input.
//! - A link is kept only when its URL passes [`crate::render::link_url`]
//!   (`https:`, no user info, no unsafe characters, at most
//!   [`crate::render::URL_MAX`] bytes); any other `[text](url)`, and every
//!   `javascript:`, `data:` or `http:` URL, stays text exactly as typed.
//! - Anything unmatched (`**` without a partner, a lone `[`) is text. HTML
//!   is never interpreted: every output escapes it.
//!
//! A body with none of these renders exactly as before: [`Doc::html`] is
//! the same `<p class="app-body">` and [`Doc::plain`] the same text.

mod html;
mod inline;

#[cfg(test)]
mod tests;

pub use html::html;
pub use inline::Inline;

/// One block of a body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Block {
    /// Text lines, each a list of inline pieces; joined by line breaks.
    Text(Vec<Vec<Inline>>),
    /// A bullet list: each item's raw marker (`- `) and content.
    Bullets(Vec<Item>),
    /// A numbered list starting at `start`.
    Numbered {
        /// The first item's number.
        start: u32,
        /// The items.
        items: Vec<Item>,
    },
}

/// One list item: its marker as typed (for the plain text) and content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `- ` or `12. ` as typed.
    pub marker: String,
    /// The content after the marker.
    pub content: Vec<Inline>,
}

/// A link found in a body, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The text shown (the URL itself for a bare URL).
    pub text: String,
    /// The `https:` URL.
    pub url: String,
}

/// A parsed body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Doc {
    /// Blocks in order.
    pub blocks: Vec<Block>,
}

/// Parses `body`. Never fails: whatever is not the format is text.
pub fn parse(body: &str) -> Doc {
    let mut blocks: Vec<Block> = Vec::new();
    for line in body.split('\n') {
        match list_marker(line) {
            Some((marker, rest, number)) => {
                let item = Item {
                    marker: marker.to_string(),
                    content: inline::parse(rest),
                };
                match (blocks.last_mut(), number) {
                    (Some(Block::Bullets(items)), None) => items.push(item),
                    (Some(Block::Numbered { items, .. }), Some(_)) => items.push(item),
                    (_, None) => blocks.push(Block::Bullets(vec![item])),
                    (_, Some(start)) => blocks.push(Block::Numbered {
                        start,
                        items: vec![item],
                    }),
                }
            }
            None => {
                let content = inline::parse(line);
                match blocks.last_mut() {
                    Some(Block::Text(lines)) => lines.push(content),
                    _ => blocks.push(Block::Text(vec![content])),
                }
            }
        }
    }
    Doc { blocks }
}

/// `(marker, rest, number)` of a list line: `- x` → `("- ", "x", None)`,
/// `3. x` → `("3. ", "x", Some(3))`.
fn list_marker(line: &str) -> Option<(&str, &str, Option<u32>)> {
    if let Some(rest) = line.strip_prefix("- ") {
        return Some((&line[..2], rest, None));
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) && line[digits..].starts_with(". ") {
        let n = line[..digits].parse().ok()?;
        return Some((&line[..digits + 2], &line[digits + 2..], Some(n)));
    }
    None
}

impl Doc {
    /// The body as plain text, for chat: markup removed, link text kept
    /// (a bare URL stays the URL), list markers and line breaks as typed.
    /// A body without markup comes back unchanged.
    pub fn plain(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for b in &self.blocks {
            match b {
                Block::Text(ls) => lines.extend(ls.iter().map(|l| inline::plain(l))),
                Block::Bullets(items) | Block::Numbered { items, .. } => lines.extend(
                    items
                        .iter()
                        .map(|i| format!("{}{}", i.marker, inline::plain(&i.content))),
                ),
            }
        }
        lines.join("\n")
    }

    /// Every link, in reading order (duplicates included).
    pub fn links(&self) -> Vec<Link> {
        let mut out = Vec::new();
        for b in &self.blocks {
            match b {
                Block::Text(ls) => ls.iter().for_each(|l| inline::links(l, &mut out)),
                Block::Bullets(items) | Block::Numbered { items, .. } => items
                    .iter()
                    .for_each(|i| inline::links(&i.content, &mut out)),
            }
        }
        out
    }

    /// The first `n` links with distinct URLs, in reading order.
    pub fn first_links(&self, n: usize) -> Vec<Link> {
        let mut out: Vec<Link> = Vec::new();
        for l in self.links() {
            if out.len() == n {
                break;
            }
            if !out.iter().any(|o| o.url == l.url) {
                out.push(l);
            }
        }
        out
    }

    /// Whether the body uses any construct (a plain body does not).
    pub fn is_plain(&self) -> bool {
        self.blocks.iter().all(|b| match b {
            Block::Text(ls) => ls.iter().all(|l| inline::is_text(l)),
            _ => false,
        })
    }
}

/// [`Doc::plain`] of `body`.
pub fn plain(body: &str) -> String {
    parse(body).plain()
}
