//! PDF attachments (#113): what an upload must be, the name kept, the
//! `Content-Disposition` it is served with, and the Mini App list.
//!
//! **The sniff.** The bytes start with `%PDF-` and hold `%%EOF`, and the
//! name ends `.pdf`. An HTML or any other file renamed `.pdf` is refused.
//! It is then scanned once, cheaply, for active content
//! ([`super::files_pdf_scan`]): script, embedded files, launch, form
//! submission, remote go-to, XFA and rich media keys refuse it, and so
//! does a string, hex string or stream left open to the end.
//!
//! **The scan is a best-effort filter.** It reads only what is not inside
//! a stream: a PDF 1.5 object stream (compressed dictionaries) can hide the
//! same keys, and it would pass. The real protection is the delivery: the
//! server never opens the file, serves it with `nosniff` and
//! `Content-Security-Policy: sandbox`, and LINE's button opens it in the
//! phone's own browser.

use maud::{Markup, html};

use super::files::{MAX_PDF_BYTES, Stored, pdfs};
use crate::render::clean_label;

/// The served content type.
pub(crate) const CONTENT_TYPE: &str = "application/pdf";

/// Longest name kept, in characters (the extension included).
const NAME_MAX: usize = 120;

/// Why an upload is not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Not a PDF, or not named `.pdf`.
    NotPdf,
    /// Script, an embedded file or a launch action found.
    Active,
    /// A string, hex string or stream left open to the end, or a
    /// malformed hex string: the scan could not read the rest.
    Unreadable,
}

/// Whether `bytes` named `file_name` is a PDF the Announcements take.
pub(crate) fn sniff(bytes: &[u8], file_name: &str) -> Result<(), Refusal> {
    let named = file_name.to_ascii_lowercase().ends_with(".pdf");
    if !named || !bytes.starts_with(b"%PDF-") || !contains(bytes, b"%%EOF") {
        return Err(Refusal::NotPdf);
    }
    match super::files_pdf_scan::scan(bytes) {
        super::files_pdf_scan::Verdict::Clean => {}
        super::files_pdf_scan::Verdict::Active => return Err(Refusal::Active),
        super::files_pdf_scan::Verdict::Unreadable => return Err(Refusal::Unreadable),
    }
    Ok(())
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The name kept for an upload: its last path part, without control or
/// invisible characters, at most [`NAME_MAX`] characters with its `.pdf`
/// kept; `document-N.pdf` when nothing is left. Shown escaped, sent in a
/// header only percent-encoded ([`disposition`]).
pub(crate) fn safe_name(raw: &str, n: u8) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = clean_label(base)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let clean = clean.trim();
    let stem = clean
        .get(..clean.len().saturating_sub(4))
        .unwrap_or_default()
        .trim();
    if stem.is_empty() {
        return format!("document-{n}.pdf");
    }
    let ext = &clean[stem.len()..].trim_start();
    if clean.chars().count() <= NAME_MAX {
        return format!("{stem}{ext}");
    }
    let keep: String = stem.chars().take(NAME_MAX - 4).collect();
    format!("{}{ext}", keep.trim_end())
}

/// `inline; filename="…"; filename*=UTF-8''…` (RFC 6266, RFC 8187): the
/// name as typed (Thai too) in `filename*`, and an ASCII fallback in
/// `filename`: the name itself when it is plain ASCII, else
/// `document-N.pdf`.
pub(crate) fn disposition(name: &str, n: u8) -> String {
    let plain = name
        .bytes()
        .all(|b| (b.is_ascii_graphic() && !matches!(b, b'"' | b'\\' | b';' | b'%')) || b == b' ');
    let fallback = if plain && !name.is_empty() {
        name.to_string()
    } else {
        format!("document-{n}.pdf")
    };
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            enc.push(char::from(b));
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("inline; filename=\"{fallback}\"; filename*=UTF-8''{enc}")
}

/// `850 KB`, `1.2 MB`: what the Mini App shows next to a name.
pub(crate) fn size_label(bytes: i64) -> String {
    const MB: i64 = 1024 * 1024;
    if bytes < MB {
        format!("{} KB", (bytes + 1023) / 1024)
    } else {
        // One decimal, rounded half up, in integers.
        let tenths = (bytes * 10 + MB / 2) / MB;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
}

/// The PDFs of `files` (#113), each a link with its name and size; `staff`
/// links the back-office route (a download), else the public path, which
/// the Mini App's script opens outside the chat app (`data-ext`).
pub(crate) fn list_html(files: &[Stored], staff: bool) -> Markup {
    let list: Vec<&Stored> = pdfs(files).collect();
    html! {
        @if !list.is_empty() {
            ul class="app-files" {
                @for f in list {
                    @let href = if staff { f.staff_path() } else { f.public_path() };
                    li {
                        a href=(href) target="_blank" rel="noopener noreferrer"
                            type=(CONTENT_TYPE) data-ext download[staff] { "📄 " (f.file_name) }
                        " " span class="size" { "(" (size_label(f.size_bytes)) ")" }
                    }
                }
            }
        }
    }
}

/// The read limit of a posted file field: a PDF slot's or an image's.
pub(crate) fn read_limit(field: &str) -> Option<usize> {
    field.starts_with("pdf").then_some(MAX_PDF_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PDF: &[u8] = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\ntrailer << >>\n%%EOF\n";

    #[test]
    fn sniff_takes_a_pdf_named_pdf() {
        assert_eq!(sniff(PDF, "ตารางสอบ.pdf"), Ok(()));
        assert_eq!(sniff(PDF, "A.PDF"), Ok(()));
        assert_eq!(sniff(PDF, "a.pdf.html"), Err(Refusal::NotPdf));
        assert_eq!(sniff(PDF, "a"), Err(Refusal::NotPdf));
        let html = b"<!doctype html><script>alert(1)</script>%%EOF";
        assert_eq!(sniff(html, "a.pdf"), Err(Refusal::NotPdf));
        assert_eq!(sniff(b"%PDF-1.4\nno end", "a.pdf"), Err(Refusal::NotPdf));
        assert_eq!(sniff(b"", "a.pdf"), Err(Refusal::NotPdf));
        assert_eq!(sniff(b" %PDF-1.4 %%EOF", "a.pdf"), Err(Refusal::NotPdf));
    }

    #[test]
    fn names_are_kept_and_made_safe() {
        assert_eq!(safe_name("ตารางสอบ ภาค 1.pdf", 1), "ตารางสอบ ภาค 1.pdf");
        assert_eq!(safe_name("C:\\Users\\x\\ประกาศ.PDF", 1), "ประกาศ.PDF");
        assert_eq!(safe_name("../../etc/a.pdf", 2), "a.pdf");
        assert_eq!(safe_name("a\u{202E}fdp.exe\u{200B}.pdf", 1), "afdp.exe.pdf");
        assert_eq!(safe_name("a\r\nb\t.pdf", 1), "ab.pdf");
        assert_eq!(safe_name(".pdf", 3), "document-3.pdf");
        assert_eq!(safe_name("  \u{200B} .pdf", 2), "document-2.pdf");
        let long = format!("{}.pdf", "ก".repeat(300));
        let kept = safe_name(&long, 1);
        assert_eq!(kept.chars().count(), NAME_MAX);
        assert!(kept.ends_with("ก.pdf"));
    }

    #[test]
    fn disposition_carries_a_thai_name() {
        assert_eq!(
            disposition("ตาราง.pdf", 1),
            "inline; filename=\"document-1.pdf\"; \
             filename*=UTF-8''%E0%B8%95%E0%B8%B2%E0%B8%A3%E0%B8%B2%E0%B8%87.pdf"
        );
        assert_eq!(
            disposition("exam 2569.pdf", 2),
            "inline; filename=\"exam 2569.pdf\"; filename*=UTF-8''exam%202569.pdf"
        );
        // Quotes, semicolons and line breaks never reach the header raw.
        let d = disposition("a\";x=1\r\n.pdf", 3);
        assert!(
            d.starts_with("inline; filename=\"document-3.pdf\"; "),
            "{d}"
        );
        assert!(!d.contains('\r') && !d.contains('\n'));
        assert!(axum::http::HeaderValue::from_str(&d).is_ok());
    }

    #[test]
    fn sizes_read_as_kb_and_mb() {
        assert_eq!(size_label(1), "1 KB");
        assert_eq!(size_label(1024), "1 KB");
        assert_eq!(size_label(1025), "2 KB");
        assert_eq!(size_label(1024 * 1024), "1.0 MB");
        assert_eq!(size_label(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(size_label(1_258_291), "1.2 MB");
    }
}
