//! Where markers close, found in linear time over one line (#108 review:
//! a 2,000-character hostile line must not cost a quadratic parse).
//!
//! The candidate closers of `**` and `*` are listed once, from the runs of
//! stars; openers only move forward, so each list is walked once. A
//! closer whose content held only stars and spaces is remembered as dead,
//! so later openers before it give up at once. The next `]` and `[` are
//! cached the same way, a bracket link's URL scan stops at the next
//! bracket or space, and a bare URL that failed marks its whole token
//! dead. The next closer of each colour (#113, `{/red}`) is cached the
//! same way too.

use super::Colour;
use crate::render::{URL_MAX, clean_label, label_or_host, link_url};

#[cfg(test)]
thread_local! {
    /// Steps taken by the parse on this thread: one per character the
    /// main loop reads plus every byte a helper scans (tests only).
    pub(super) static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `n` steps of work (a no-op outside tests).
#[inline]
pub(super) fn step(n: usize) {
    #[cfg(test)]
    STEPS.with(|s| s.set(s.get() + n));
    #[cfg(not(test))]
    let _ = n;
}

/// The closers of one line (or one Bold/Italic content).
pub(super) struct Scan<'a> {
    s: &'a str,
    /// Where a closing `**` starts: the last two stars of a run that
    /// follows a non-space.
    bold: Vec<usize>,
    /// Where a closing `*` is: the last star of a run of odd length that
    /// follows a non-space (or holds a `**` before it).
    italic: Vec<usize>,
    next_bold: usize,
    next_italic: usize,
    dead_bold: Option<usize>,
    dead_italic: Option<usize>,
    /// The last search for `]` / `[`: its answer, valid for any later
    /// start up to the position found.
    rb: Option<Option<usize>>,
    lb: Option<Option<usize>>,
    /// The last search for each colour's closer, the same way.
    colour_close: [Option<Option<usize>>; 5],
    /// No bare URL starts before this byte (a failed token).
    bare_dead_until: usize,
}

fn solid_before(s: &str, i: usize) -> bool {
    s[..i]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace())
}

fn starts_solid(s: &str) -> bool {
    s.chars().next().is_some_and(|c| !c.is_whitespace())
}

/// Emphasis holds something besides stars and spaces.
fn has_words(s: &str) -> bool {
    step(s.len());
    s.chars().any(|c| c != '*' && !c.is_whitespace())
}

impl<'a> Scan<'a> {
    pub(super) fn new(s: &'a str) -> Self {
        let (mut bold, mut italic) = (Vec::new(), Vec::new());
        let b = s.as_bytes();
        let mut p = 0;
        while p < b.len() {
            if b[p] != b'*' {
                p += 1;
                continue;
            }
            let r = b[p..].iter().take_while(|&&c| c == b'*').count();
            let solid = solid_before(s, p);
            if r >= 2 && solid {
                bold.push(p + r - 2);
            }
            if r % 2 == 1 && (r > 1 || solid) {
                italic.push(p + r - 1);
            }
            p += r;
        }
        Self {
            s,
            bold,
            italic,
            next_bold: 0,
            next_italic: 0,
            dead_bold: None,
            dead_italic: None,
            rb: None,
            lb: None,
            colour_close: [None; 5],
            bare_dead_until: 0,
        }
    }

    /// Where the `**` closing a bold whose content starts at `from` is.
    pub(super) fn close_bold(&mut self, from: usize) -> Option<usize> {
        if !starts_solid(&self.s[from..]) {
            return None;
        }
        while self.bold.get(self.next_bold).is_some_and(|&c| c <= from) {
            self.next_bold += 1;
        }
        let c = *self.bold.get(self.next_bold)?;
        Self::check(self.s, from, c, &mut self.dead_bold)
    }

    /// Where the `*` closing an italic whose content starts at `from` is.
    pub(super) fn close_italic(&mut self, from: usize) -> Option<usize> {
        let body = &self.s[from..];
        if !starts_solid(body) || body.starts_with('*') {
            return None;
        }
        while self
            .italic
            .get(self.next_italic)
            .is_some_and(|&c| c <= from)
        {
            self.next_italic += 1;
        }
        let c = *self.italic.get(self.next_italic)?;
        Self::check(self.s, from, c, &mut self.dead_italic)
    }

    fn check(s: &str, from: usize, c: usize, dead: &mut Option<usize>) -> Option<usize> {
        if *dead == Some(c) {
            return None;
        }
        if has_words(&s[from..c]) {
            Some(c)
        } else {
            *dead = Some(c);
            None
        }
    }

    fn find_cached(
        s: &str,
        cache: &mut Option<Option<usize>>,
        from: usize,
        b: char,
    ) -> Option<usize> {
        let mut buf = [0u8; 4];
        Self::find_str_cached(s, cache, from, b.encode_utf8(&mut buf))
    }

    /// Where `pat` next starts at or after `from`. The answer of the last
    /// search holds for any later `from` up to the position found (or for
    /// every one, when nothing was found), so each byte is scanned once.
    fn find_str_cached(
        s: &str,
        cache: &mut Option<Option<usize>>,
        from: usize,
        pat: &str,
    ) -> Option<usize> {
        if let Some(found) = *cache
            && found.is_none_or(|p| p >= from)
        {
            return found;
        }
        let found = s[from..].find(pat).map(|k| from + k);
        step(found.map_or(s.len(), |p| p + 1) - from);
        *cache = Some(found);
        found
    }

    /// `{name}text{/name}` opening at `i` (#113): (colour, where the text
    /// starts, where its closer starts), when `name` is in the palette and
    /// the text is not empty. Anything else (`{pink}`, `{red}` without its
    /// closer) is text.
    pub(super) fn colour(&mut self, i: usize) -> Option<(Colour, usize, usize)> {
        let rest = &self.s[i..];
        step(9);
        let c = Colour::ALL
            .into_iter()
            .find(|c| rest.starts_with(c.open()))?;
        let from = i + c.open().len();
        let close =
            Self::find_str_cached(self.s, &mut self.colour_close[c.index()], from, c.close())?;
        (close > from).then_some((c, from, close))
    }

    /// `[text](url)` opening at `i`: (label, url, end), when the text holds
    /// no bracket, the URL's parentheses balance, and the URL passes
    /// [`link_url`]. The label is the text without invisible characters
    /// ([`label_or_host`]), or the URL's host when nothing visible is
    /// left: a link, and the URL button made from it, never has an empty
    /// label.
    pub(super) fn bracket_link(&mut self, i: usize) -> Option<(String, &'a str, usize)> {
        let s = self.s;
        let close = Self::find_cached(s, &mut self.rb, i + 1, ']')?;
        if Self::find_cached(s, &mut self.lb, i + 1, '[').is_some_and(|p| p < close) {
            return None;
        }
        let text = &s[i + 1..close];
        if !s[close + 1..].starts_with('(') {
            return None;
        }
        let start = close + 2;
        let mut depth = 0usize;
        let mut end = None;
        let mut scanned = 0;
        for (k, &c) in s.as_bytes()[start..].iter().enumerate() {
            scanned = k + 1;
            if k > URL_MAX || c.is_ascii_whitespace() || !c.is_ascii() || matches!(c, b'[' | b']') {
                break;
            }
            match c {
                b'(' => depth += 1,
                b')' if depth == 0 => {
                    end = Some(start + k);
                    break;
                }
                b')' => depth -= 1,
                _ => {}
            }
        }
        step(scanned);
        let end = end?;
        let url = &s[start..end];
        step(url.len() + text.len());
        link_url(url).then(|| (label_or_host(text, url), url, end + 1))
    }

    /// `![alt](image:N)` opening at `i` (#109): (alt, slot, end), when the
    /// alt holds no bracket and `N` is one digit from 1 to 9. The alt
    /// loses invisible characters ([`clean_label`]). Its `]` and `[` come
    /// from the same caches as a link's, so a run of `![` costs one scan.
    pub(super) fn image(&mut self, i: usize) -> Option<(String, u8, usize)> {
        let s = self.s;
        if !s[i..].starts_with("![") {
            return None;
        }
        let open = i + 1;
        let close = Self::find_cached(s, &mut self.rb, open + 1, ']')?;
        if Self::find_cached(s, &mut self.lb, open + 1, '[').is_some_and(|p| p < close) {
            return None;
        }
        let tail = s[close + 1..].strip_prefix("(image:")?.as_bytes();
        step(9);
        let n = match tail {
            [d @ b'1'..=b'9', b')', ..] => d - b'0',
            _ => return None,
        };
        let alt = &s[open + 1..close];
        step(alt.len());
        Some((clean_label(alt), n, close + 10))
    }

    /// The bare URL at `i`, if one starts a word there and passes
    /// [`link_url`]: up to a space, a quote, an angle bracket or a
    /// non-ASCII character, without trailing `.,;:!?*` and without a `)`
    /// that closes nothing in it.
    pub(super) fn bare_url(&mut self, i: usize) -> Option<&'a str> {
        let s = self.s;
        if i < self.bare_dead_until || !s[i..].starts_with("https://") || !word_start(s, i) {
            return None;
        }
        let token_end = s[i..]
            .find(|c: char| {
                c.is_whitespace() || !c.is_ascii() || matches!(c, '<' | '>' | '"' | '\'')
            })
            .map_or(s.len(), |k| i + k);
        step(token_end - i);
        let b = &s.as_bytes()[i..token_end];
        let opens = b.iter().filter(|&&c| c == b'(').count();
        let mut closes = b.iter().filter(|&&c| c == b')').count();
        let mut len = b.len();
        while len > 0 {
            match b[len - 1] {
                b'.' | b',' | b';' | b':' | b'!' | b'?' | b'*' => len -= 1,
                b')' if closes > opens => {
                    closes -= 1;
                    len -= 1;
                }
                _ => break,
            }
        }
        let url = &s[i..i + len];
        if link_url(url) {
            Some(url)
        } else {
            self.bare_dead_until = token_end;
            None
        }
    }
}

/// A bare URL starts a word: not glued to ASCII letters, digits or URL
/// punctuation before it.
fn word_start(s: &str, i: usize) -> bool {
    s[..i].chars().next_back().is_none_or(|c| {
        !(c.is_ascii_alphanumeric() || matches!(c, '/' | ':' | '@' | '.' | '-' | '_' | '%'))
    })
}
