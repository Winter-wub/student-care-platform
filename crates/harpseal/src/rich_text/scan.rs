//! Where markers close, found in linear time over one line (#108 review:
//! a 2,000-character hostile line must not cost a quadratic parse).
//!
//! The candidate closers of `**` and `*` are listed once, from the runs of
//! stars; openers only move forward, so each list is walked once. A
//! closer whose content held only stars and spaces is remembered as dead,
//! so later openers before it give up at once. The next `]` and `[` are
//! cached the same way, a bracket link's URL scan stops at the next
//! bracket or space, and a bare URL that failed marks its whole token
//! dead.

use crate::render::{URL_MAX, link_url};

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
        if let Some(found) = *cache
            && found.is_none_or(|p| p >= from)
        {
            return found;
        }
        let found = s[from..].find(b).map(|k| from + k);
        *cache = Some(found);
        found
    }

    /// `[text](url)` opening at `i`: (text, url, end), when the text is not
    /// blank and holds no bracket, the URL's parentheses balance, and the
    /// URL passes [`link_url`].
    pub(super) fn bracket_link(&mut self, i: usize) -> Option<(&'a str, &'a str, usize)> {
        let s = self.s;
        let close = Self::find_cached(s, &mut self.rb, i + 1, ']')?;
        if Self::find_cached(s, &mut self.lb, i + 1, '[').is_some_and(|p| p < close) {
            return None;
        }
        let text = &s[i + 1..close];
        if text.trim().is_empty() || !s[close + 1..].starts_with('(') {
            return None;
        }
        let start = close + 2;
        let mut depth = 0usize;
        let mut end = None;
        for (k, &c) in s.as_bytes()[start..].iter().enumerate() {
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
        let end = end?;
        let url = &s[start..end];
        link_url(url).then_some((text, url, end + 1))
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
