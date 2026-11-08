//! The active-content scan of an uploaded PDF (#113): one linear pass over
//! the bytes, reading PDF tokens just far enough to find names.
//!
//! **A best-effort filter, not the protection.** It turns away the plain
//! cases (a form with a script, an embedded file) before they are stored.
//! What protects a Student is how a PDF is delivered: it is served with
//! `Content-Security-Policy: sandbox` and `nosniff`, never opened by the
//! server, and LINE's button opens it in the phone's own browser.
//!
//! - A literal string `( … )` (nested parentheses, `\` escapes), a hex
//!   string `< … >` and a `%` comment are skipped: a name or the word
//!   `stream` inside them is not a token.
//! - `stream` is a stream only right after a dictionary's `>>` (white
//!   space between allowed) and followed by an end of line; its data is
//!   skipped to the next `endstream`.
//! - Every other name (`/Name`, `#xx` escapes decoded) is compared with
//!   [`ACTIVE`].
//! - A string, hex string or stream still open at the end of the file
//!   would hide everything after it, so it refuses the file
//!   ([`Verdict::Unreadable`]), as does a hex string holding anything but
//!   hex digits and white space.
//!
//! Known limit: dictionaries compressed inside an object stream (PDF 1.5)
//! are not read.

/// What the scan found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// No [`ACTIVE`] key outside strings, comments and streams.
    Clean,
    /// An [`ACTIVE`] key.
    Active,
    /// A string, hex string or stream left open, or a malformed hex
    /// string: the rest of the file could not be read.
    Unreadable,
}

/// Keys that run something, carry or fetch another file, or send data.
const ACTIVE: [&[u8]; 9] = [
    b"JavaScript",
    b"JS",
    b"Launch",
    b"EmbeddedFile",
    b"EmbeddedFiles",
    b"SubmitForm",
    b"GoToR",
    b"XFA",
    b"RichMedia",
];

#[cfg(test)]
thread_local! {
    /// Bytes the scan read on this thread (tests only).
    static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `n` bytes read (a no-op outside tests).
#[inline]
fn step(n: usize) {
    #[cfg(test)]
    STEPS.with(|s| s.set(s.get() + n));
    #[cfg(not(test))]
    let _ = n;
}

/// White space (PDF 32000-1, 7.2.2).
fn space(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

/// White space and delimiters end a name.
fn ends_name(b: u8) -> bool {
    space(b) || b"()<>[]{}/%".contains(&b)
}

fn hex(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// The scan of `b`. Each byte is read once.
pub(super) fn scan(b: &[u8]) -> Verdict {
    let mut i = 0;
    // Only white space since the last `>>`.
    let mut after_dict = false;
    while i < b.len() {
        step(1);
        let c = b[i];
        if space(c) {
            i += 1;
            continue;
        }
        let next = if c == b's' && after_dict && stream_keyword(b, i) {
            after_dict = false;
            skip_stream(b, i + b"stream".len())
        } else {
            after_dict = false;
            match c {
                b'%' => Some(skip_line(b, i)),
                b'(' => skip_string(b, i),
                b'<' if b.get(i + 1) == Some(&b'<') => Some(i + 2),
                b'>' if b.get(i + 1) == Some(&b'>') => {
                    after_dict = true;
                    Some(i + 2)
                }
                b'<' => skip_hex(b, i + 1),
                b'/' => match name(b, i + 1) {
                    (true, _) => return Verdict::Active,
                    (false, next) => Some(next),
                },
                _ => Some(i + 1),
            }
        };
        let Some(next) = next else {
            return Verdict::Unreadable;
        };
        i = next;
    }
    Verdict::Clean
}

/// `stream` at `i`, followed by an end of line.
fn stream_keyword(b: &[u8], i: usize) -> bool {
    let after = &b[i..];
    after.starts_with(b"stream\n") || after.starts_with(b"stream\r\n")
}

/// Past the next `endstream` from `from`; `None` when there is none.
fn skip_stream(b: &[u8], from: usize) -> Option<usize> {
    const END: &[u8] = b"endstream";
    let found = b[from..].windows(END.len()).position(|w| w == END);
    step(found.map_or(b.len() - from, |k| k + END.len()));
    found.map(|k| from + k + END.len())
}

/// Past the end of the line holding the comment at `i` (a comment may
/// end the file).
fn skip_line(b: &[u8], i: usize) -> usize {
    let found = b[i..].iter().position(|&x| x == b'\n' || x == b'\r');
    step(found.map_or(b.len() - i, |k| k + 1));
    found.map_or(b.len(), |k| i + k + 1)
}

/// Past the literal string opening at `i`: balanced parentheses, `\`
/// escaping the next byte; `None` when it never closes.
fn skip_string(b: &[u8], i: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    step(j + 1 - i);
                    return Some(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    step(b.len() - i);
    None
}

/// Past the `>` of the hex string whose digits start at `from`; `None`
/// when it never closes or holds anything but hex digits and white space.
fn skip_hex(b: &[u8], from: usize) -> Option<usize> {
    for (k, &x) in b[from..].iter().enumerate() {
        if x == b'>' {
            step(k + 1);
            return Some(from + k + 1);
        }
        if !(x.is_ascii_hexdigit() || space(x)) {
            step(k + 1);
            return None;
        }
    }
    step(b.len() - from);
    None
}

/// The name starting at `i` (after its `/`): whether it is [`ACTIVE`],
/// and where it ends.
fn name(b: &[u8], i: usize) -> (bool, usize) {
    let mut buf = [0u8; 16];
    let mut len = 0usize;
    let mut j = i;
    while j < b.len() && !ends_name(b[j]) {
        let mut c = b[j];
        j += 1;
        if c == b'#'
            && let (Some(h), Some(l)) = (
                b.get(j).copied().and_then(hex),
                b.get(j + 1).copied().and_then(hex),
            )
        {
            c = h * 16 + l;
            j += 2;
        }
        if len < buf.len() {
            buf[len] = c;
        }
        len += 1;
    }
    step(j - i);
    (len <= buf.len() && ACTIVE.contains(&&buf[..len]), j)
}

#[cfg(test)]
mod tests {
    use super::{STEPS, Verdict, scan};

    fn pdf(body: &str) -> Vec<u8> {
        format!("%PDF-1.7\n{body}\n%%EOF").into_bytes()
    }

    #[test]
    fn active_keys_are_found() {
        for key in [
            "/JavaScript",
            "/JS",
            "/J#53",
            "/Ja#76aScript",
            "/Launch",
            "/EmbeddedFiles",
            "/EmbeddedFile",
            "/SubmitForm",
            "/GoToR",
            "/XFA",
            "/RichMedia",
        ] {
            let b = pdf(&format!(
                "1 0 obj << /OpenAction << /S {key} (x) >> >> endobj"
            ));
            assert_eq!(scan(&b), Verdict::Active, "{key}");
        }
    }

    #[test]
    fn a_fake_stream_in_a_string_or_comment_hides_nothing() {
        // Round 1's sample: `stream` and `endstream` inside strings.
        let b = pdf(
            "1 0 obj (x stream\n) endobj 2 0 obj << /S /JavaScript /JS (app.alert(1)) >> \
             endobj 3 0 obj (endstream) endobj",
        );
        assert_eq!(scan(&b), Verdict::Active);
        for body in [
            "% >> stream\n<< /S /JS >>\n% endstream",
            "(a (nested) >> stream\n) << /JS 1 >> (endstream)",
            "(esc \\) >> stream\n) /Launch (endstream)",
            "<73747265616d> /XFA",
            "1 0 obj stream\n/JS\nendstream",
        ] {
            assert_eq!(scan(&pdf(body)), Verdict::Active, "{body}");
        }
    }

    #[test]
    fn whatever_stays_open_to_the_end_is_refused() {
        // Round 2's samples: an unreferenced `(` or `<` before the script
        // would swallow it.
        for body in [
            "0 0 obj (\n1 0 obj << /OpenAction 2 0 R >> endobj\n\
             2 0 obj << /S /JavaScript /JS (app.alert(1)) >> endobj",
            "0 0 obj <\n1 0 obj << /OpenAction << /S /JavaScript /JS (x) >> >> endobj",
            "(never closed /JS",
            "<< >>\nstream\n/JS",
            "<< /Length 3 >>\nstream\nabc",
            "(a (b) c",
            "<4a53",
        ] {
            assert_eq!(scan(&pdf(body)), Verdict::Unreadable, "{body}");
        }
    }

    #[test]
    fn a_hex_string_holds_hex_digits_and_space_only() {
        assert_eq!(scan(&pdf("<4a 53\n0F> /Type")), Verdict::Clean);
        for body in ["<4a5g>", "<4a /JS>", "<(x)>", "<4a\u{1}>"] {
            assert_eq!(scan(&pdf(body)), Verdict::Unreadable, "{body}");
        }
    }

    #[test]
    fn real_streams_strings_and_longer_names_pass() {
        for body in [
            "/JSON /JavaScripts /Launcher /EmbeddedFilesX /XFAx",
            "<< /Length 9 >>\nstream\n/JS /Launch\nendstream",
            "<< /Length 9 >>  \r\n stream\r\n(/JavaScript\nendstream /Type",
            "(a /JS in text) (and \\( /Launch)",
            "% a comment naming /JavaScript\n/Type /Page",
            "<4a53> /Type",
        ] {
            assert_eq!(scan(&pdf(body)), Verdict::Clean, "{body}");
        }
        // After a real stream, keys are read again.
        assert_eq!(
            scan(&pdf("<< >>\nstream\nx\nendstream /JS")),
            Verdict::Active
        );
        // A comment may end the file.
        assert_eq!(scan(b"%PDF-1.7\n%%EOF"), Verdict::Clean);
    }

    /// Bytes read for `b`.
    fn steps(b: &[u8]) -> usize {
        STEPS.with(|c| c.set(0));
        let _ = scan(b);
        STEPS.with(std::cell::Cell::get)
    }

    /// Deterministic, whatever the machine's load: at most 4 bytes read
    /// per byte of input for each hostile pattern, at 64 KB and at 8 times
    /// that. A skip that rescanned would blow the bound at once.
    #[test]
    fn hostile_inputs_scan_in_linear_steps() {
        for unit in [
            "(",
            "()",
            "(()",
            ">>\nstream\n",
            ">>\nstream\nendstream",
            "%",
            "%\n",
            "<",
            "<>",
            "<<",
            "/J",
            "/",
            "\\",
            ">> s",
            "/JavaScriptX ",
        ] {
            for size in [64 * 1024, 512 * 1024] {
                let b = unit.repeat(size / unit.len() + 1);
                let n = steps(b.as_bytes());
                assert!(n <= 4 * b.len(), "{unit:?} x{}: {n} steps", b.len());
            }
        }
    }
}
