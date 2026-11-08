//! The active-content scan of an uploaded PDF (#113): one linear pass over
//! the bytes, reading PDF tokens just far enough to find names.
//!
//! - A literal string `( … )` (nested parentheses, `\` escapes), a hex
//!   string `< … >` and a `%` comment are skipped: a name or the word
//!   `stream` inside them is not a token.
//! - `stream` is a stream only right after a dictionary's `>>` (white
//!   space between allowed) and followed by an end of line; its data is
//!   skipped to the next `endstream`.
//! - Every other name (`/Name`, `#xx` escapes decoded) is compared with
//!   [`ACTIVE`].
//!
//! Known limit: dictionaries compressed inside an object stream (PDF 1.5)
//! are not read.

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

/// Whether `b` holds an [`ACTIVE`] key outside strings, comments and
/// streams. Each byte is read once.
pub(super) fn active(b: &[u8]) -> bool {
    let mut i = 0;
    // Only white space since the last `>>`.
    let mut after_dict = false;
    while i < b.len() {
        let c = b[i];
        if space(c) {
            i += 1;
            continue;
        }
        if c == b's' && after_dict && stream_keyword(b, i) {
            i = skip_stream(b, i + b"stream".len());
            after_dict = false;
            continue;
        }
        after_dict = false;
        match c {
            b'%' => i = skip_line(b, i),
            b'(' => i = skip_string(b, i),
            b'<' if b.get(i + 1) == Some(&b'<') => i += 2,
            b'>' if b.get(i + 1) == Some(&b'>') => {
                i += 2;
                after_dict = true;
            }
            b'<' => {
                i = b[i..]
                    .iter()
                    .position(|&x| x == b'>')
                    .map_or(b.len(), |k| i + k + 1)
            }
            b'/' => {
                let (hit, next) = name(b, i + 1);
                if hit {
                    return true;
                }
                i = next;
            }
            _ => i += 1,
        }
    }
    false
}

/// `stream` at `i`, followed by an end of line.
fn stream_keyword(b: &[u8], i: usize) -> bool {
    let after = &b[i..];
    after.starts_with(b"stream\n") || after.starts_with(b"stream\r\n")
}

/// Past the next `endstream` from `from` (the end when there is none).
fn skip_stream(b: &[u8], from: usize) -> usize {
    const END: &[u8] = b"endstream";
    b[from..]
        .windows(END.len())
        .position(|w| w == END)
        .map_or(b.len(), |k| from + k + END.len())
}

/// Past the end of the line holding the comment at `i`.
fn skip_line(b: &[u8], i: usize) -> usize {
    b[i..]
        .iter()
        .position(|&x| x == b'\n' || x == b'\r')
        .map_or(b.len(), |k| i + k + 1)
}

/// Past the literal string opening at `i`: balanced parentheses, `\`
/// escaping the next byte; the end when it never closes.
fn skip_string(b: &[u8], i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len()
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
    (len <= buf.len() && ACTIVE.contains(&&buf[..len]), j)
}

#[cfg(test)]
mod tests {
    use super::active;

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
            assert!(active(&b), "{key}");
        }
    }

    #[test]
    fn a_fake_stream_in_a_string_or_comment_hides_nothing() {
        // The reviewer's sample: `stream` and `endstream` inside strings.
        let b = pdf(
            "1 0 obj (x stream\n) endobj 2 0 obj << /S /JavaScript /JS (app.alert(1)) >> \
             endobj 3 0 obj (endstream) endobj",
        );
        assert!(active(&b));
        for body in [
            "% >> stream\n<< /S /JS >>\n% endstream",
            "(a (nested) >> stream\n) << /JS 1 >> (endstream)",
            "(esc \\) >> stream\n) /Launch (endstream)",
            "<73747265616d> /XFA",
            "1 0 obj stream\n/JS\nendstream",
        ] {
            assert!(active(&pdf(body)), "{body}");
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
            assert!(!active(&pdf(body)), "{body}");
        }
        // After a real stream, keys are read again.
        assert!(active(&pdf("<< >>\nstream\nx\nendstream /JS")));
        // An unclosed string or stream runs to the end, finding nothing.
        assert!(!active(&pdf("(never closed /JS")));
        assert!(!active(&pdf("<< >>\nstream\n/JS")));
    }

    #[test]
    fn hostile_inputs_scan_in_linear_time() {
        // Each would be quadratic if a skip rescanned: 4 MB in well
        // under a second even unoptimised.
        for unit in ["(", ">>\nstream\n", "%", "<", "/J", "\\", "(()"] {
            let b = unit.repeat(4 * 1024 * 1024 / unit.len());
            let t = std::time::Instant::now();
            let _ = active(b.as_bytes());
            assert!(t.elapsed().as_secs() < 5, "{unit:?}");
        }
    }
}
