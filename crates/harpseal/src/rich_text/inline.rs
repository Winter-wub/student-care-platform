//! Inline pieces of one line: bold, italic, links, text. The parse is
//! linear in the line's length: closers are found through
//! [`super::scan::Scan`], which never rescans what a failed opener read.

use super::Link;
use super::scan::{Scan, step};

/// One piece of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Inline {
    /// Text as typed.
    Text(String),
    /// `**…**`.
    Bold(Vec<Inline>),
    /// `*…*`.
    Italic(Vec<Inline>),
    /// `[text](https://…)` or a bare `https://…`.
    Link {
        /// The text shown.
        text: String,
        /// The `https:` URL ([`crate::render::link_url`] holds).
        url: String,
    },
}

/// What may open inside the current piece: a kind never nests in itself,
/// and a link's text is plain.
#[derive(Clone, Copy)]
struct Allow {
    bold: bool,
    italic: bool,
    link: bool,
}

/// The pieces of `line`.
pub(super) fn parse(line: &str) -> Vec<Inline> {
    parse_with(
        line,
        Allow {
            bold: true,
            italic: true,
            link: true,
        },
    )
}

/// One pass over `s`; a Bold or Italic parses its content (a disjoint
/// slice) one level down, at most three levels.
fn parse_with(s: &str, allow: Allow) -> Vec<Inline> {
    let mut scan = Scan::new(s);
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while let Some(ch) = s[i..].chars().next() {
        step(1);
        let rest = &s[i..];
        if rest.starts_with("**") {
            if let Some(end) = allow.bold.then(|| scan.close_bold(i + 2)).flatten() {
                flush(&mut text, &mut out);
                let inner = Allow {
                    bold: false,
                    ..allow
                };
                out.push(Inline::Bold(parse_with(&s[i + 2..end], inner)));
                i = end + 2;
            } else {
                // Both stars are text: the second never opens an italic.
                text.push_str("**");
                i += 2;
            }
            continue;
        }
        if ch == '*'
            && let Some(end) = allow.italic.then(|| scan.close_italic(i + 1)).flatten()
        {
            flush(&mut text, &mut out);
            let inner = Allow {
                italic: false,
                ..allow
            };
            out.push(Inline::Italic(parse_with(&s[i + 1..end], inner)));
            i = end + 1;
            continue;
        }
        if allow.link
            && ch == '['
            && let Some((t, url, end)) = scan.bracket_link(i)
        {
            flush(&mut text, &mut out);
            out.push(Inline::Link {
                text: t,
                url: url.to_string(),
            });
            i = end;
            continue;
        }
        if allow.link
            && ch == 'h'
            && let Some(url) = scan.bare_url(i)
        {
            flush(&mut text, &mut out);
            out.push(Inline::Link {
                text: url.to_string(),
                url: url.to_string(),
            });
            i += url.len();
            continue;
        }
        text.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut text, &mut out);
    out
}

fn flush(text: &mut String, out: &mut Vec<Inline>) {
    if !text.is_empty() {
        out.push(Inline::Text(std::mem::take(text)));
    }
}

/// The text of `pieces`, markup removed, link text kept.
pub(super) fn plain(pieces: &[Inline]) -> String {
    let mut out = String::new();
    for p in pieces {
        match p {
            Inline::Text(t) => out.push_str(t),
            Inline::Bold(c) | Inline::Italic(c) => out.push_str(&plain(c)),
            Inline::Link { text, .. } => out.push_str(text),
        }
    }
    out
}

/// Appends the links of `pieces` to `out`, in order.
pub(super) fn links(pieces: &[Inline], out: &mut Vec<Link>) {
    for p in pieces {
        match p {
            Inline::Bold(c) | Inline::Italic(c) => links(c, out),
            Inline::Link { text, url } => out.push(Link {
                text: text.clone(),
                url: url.clone(),
            }),
            Inline::Text(_) => {}
        }
    }
}

/// Only text, no markup.
pub(super) fn is_text(pieces: &[Inline]) -> bool {
    pieces.iter().all(|p| matches!(p, Inline::Text(_)))
}
