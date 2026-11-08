//! Inline pieces of one line: bold, italic, links, text.

use super::Link;
use crate::render::link_url;

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
        /// The `https:` URL ([`link_url`] holds).
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

fn parse_with(s: &str, allow: Allow) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while let Some(ch) = s[i..].chars().next() {
        let rest = &s[i..];
        if rest.starts_with("**") {
            if let Some(end) = allow.bold.then(|| close_bold(s, i + 2)).flatten() {
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
            && let Some(end) = allow.italic.then(|| close_italic(s, i + 1)).flatten()
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
            && let Some((t, url, len)) = bracket_link(rest)
        {
            flush(&mut text, &mut out);
            out.push(Inline::Link {
                text: t.to_string(),
                url: url.to_string(),
            });
            i += len;
            continue;
        }
        if allow.link && rest.starts_with("https://") && word_start(s, i) {
            let url = &rest[..bare_len(rest)];
            if link_url(url) {
                flush(&mut text, &mut out);
                out.push(Inline::Link {
                    text: url.to_string(),
                    url: url.to_string(),
                });
                i += url.len();
                continue;
            }
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

fn starts_solid(s: &str) -> bool {
    s.chars().next().is_some_and(|c| !c.is_whitespace())
}

fn ends_solid(s: &str) -> bool {
    s.chars().next_back().is_some_and(|c| !c.is_whitespace())
}

/// Where the `**` closing a bold opened before `from` starts: the first
/// `**` after a non-space (the last two of `***`, so an italic inside
/// closes first). The content must start with a non-space.
fn close_bold(s: &str, from: usize) -> Option<usize> {
    let body = &s[from..];
    if !starts_solid(body) {
        return None;
    }
    body.match_indices("**")
        .map(|(k, _)| k)
        .find(|&k| k > 0 && ends_solid(&body[..k]))
        .map(|k| {
            if body[k..].starts_with("***") {
                from + k + 1
            } else {
                from + k
            }
        })
        .filter(|&end| has_words(&s[from..end]))
}

/// Emphasis holds something besides stars and spaces.
fn has_words(s: &str) -> bool {
    s.chars().any(|c| c != '*' && !c.is_whitespace())
}

/// Where the `*` closing an italic opened before `from` is: the first
/// lone `*` after a non-space; a `**` inside is a bold's and skipped.
fn close_italic(s: &str, from: usize) -> Option<usize> {
    let body = &s[from..];
    if !starts_solid(body) || body.starts_with('*') {
        return None;
    }
    let b = body.as_bytes();
    let mut k = 1;
    while k < b.len() {
        if b[k] == b'*' {
            if b.get(k + 1) == Some(&b'*') {
                k += 2;
                continue;
            }
            if ends_solid(&body[..k]) {
                return has_words(&body[..k]).then_some(from + k);
            }
        }
        k += 1;
    }
    None
}

/// `[text](url)` at the start of `s`: (text, url, bytes used), when the
/// text is not blank and holds no bracket and the URL passes [`link_url`].
fn bracket_link(s: &str) -> Option<(&str, &str, usize)> {
    let close = s.find(']')?;
    let text = &s[1..close];
    if text.trim().is_empty() || text.contains('[') {
        return None;
    }
    let after = s[close + 1..].strip_prefix('(')?;
    let end = after.find(')')?;
    let url = &after[..end];
    link_url(url).then_some((text, url, close + 2 + end + 1))
}

/// A bare URL starts a word: not glued to ASCII letters, digits or URL
/// punctuation before it.
fn word_start(s: &str, i: usize) -> bool {
    s[..i].chars().next_back().is_none_or(|c| {
        !(c.is_ascii_alphanumeric() || matches!(c, '/' | ':' | '@' | '.' | '-' | '_' | '%'))
    })
}

/// Length of the bare URL at the start of `s`: up to a space, a quote, an
/// angle bracket or a non-ASCII character, without trailing `.,;:!?*` and
/// without a `)` that closes nothing in it.
fn bare_len(s: &str) -> usize {
    let end = s
        .find(|c: char| c.is_whitespace() || !c.is_ascii() || matches!(c, '<' | '>' | '"' | '\''))
        .unwrap_or(s.len());
    let mut url = &s[..end];
    loop {
        let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '*']);
        let unbalanced =
            trimmed.ends_with(')') && trimmed.matches(')').count() > trimmed.matches('(').count();
        url = if unbalanced {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };
        if !unbalanced {
            return url.len();
        }
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
