//! The checks of `/admin/branding` (#114): the display name, the primary
//! colour (`#rrggbb`, and white text on it readable: a WCAG 2 contrast of
//! at least [`MIN_CONTRAST`]) and the logo (PNG or JPEG told by its bytes,
//! at most [`LOGO_MAX_BYTES`] and [`LOGO_MAX_PX`] a side).
//!
//! A refusal is a catalog key and its `{placeholder}` values, shown in the
//! Staff member's Locale next to the field.

/// Longest display name, in characters.
pub(crate) const NAME_MAX_CHARS: usize = 80;
/// Least contrast of white text on the primary colour (WCAG AA, normal text).
pub(crate) const MIN_CONTRAST: f64 = 4.5;
/// Largest logo file.
pub(crate) const LOGO_MAX_BYTES: usize = 256 * 1024;
/// Largest logo side, px.
pub(crate) const LOGO_MAX_PX: u32 = 1024;
/// Smallest logo side, px.
pub(crate) const LOGO_MIN_PX: u32 = 16;

/// Why a value was refused: a catalog key and its placeholder values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) key: &'static str,
    pub(crate) args: Vec<(&'static str, String)>,
}

impl Refused {
    fn new(key: &'static str) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }
    fn arg(mut self, name: &'static str, value: impl ToString) -> Self {
        self.args.push((name, value.to_string()));
        self
    }
    /// The text in `t`'s Locale, placeholders filled.
    pub(crate) fn text(&self, t: impl Fn(&str) -> String) -> String {
        let mut s = t(self.key);
        for (n, v) in &self.args {
            s = s.replace(&format!("{{{n}}}"), v);
        }
        s
    }
}

/// The name to save: trimmed, 1 to [`NAME_MAX_CHARS`] characters, no
/// control characters.
pub(crate) fn name(v: &str) -> Result<String, Refused> {
    let v = v.trim();
    if v.is_empty() {
        return Err(Refused::new("kernel.branding.err_name_empty"));
    }
    if v.chars().count() > NAME_MAX_CHARS {
        return Err(Refused::new("kernel.branding.err_name_long").arg("max", NAME_MAX_CHARS));
    }
    if v.chars().any(char::is_control) {
        return Err(Refused::new("kernel.branding.err_name_control"));
    }
    Ok(v.to_string())
}

/// `#rrggbb` (lowercase) as red, green, blue.
fn rgb(v: &str) -> Option<[u8; 3]> {
    let hex = v.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

/// WCAG 2 contrast ratio of white on `c`.
pub(crate) fn contrast_with_white(c: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let l = 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
    1.05 / (l + 0.05)
}

/// The colour to save, `#rrggbb` lowercase, if white text on it reaches
/// [`MIN_CONTRAST`].
pub(crate) fn color(v: &str) -> Result<String, Refused> {
    let v = v.trim().to_ascii_lowercase();
    let Some(c) = rgb(&v) else {
        return Err(Refused::new("kernel.branding.err_color"));
    };
    let ratio = contrast_with_white(c);
    if ratio < MIN_CONTRAST {
        return Err(Refused::new("kernel.branding.err_contrast")
            .arg("ratio", format!("{ratio:.1}"))
            .arg("min", format!("{MIN_CONTRAST:.1}")));
    }
    Ok(v)
}

/// A logo told by its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogoKind {
    Png,
    Jpeg,
}

impl LogoKind {
    pub(crate) fn content_type(self) -> &'static str {
        match self {
            LogoKind::Png => "image/png",
            LogoKind::Jpeg => "image/jpeg",
        }
    }
    pub(crate) fn ext(self) -> &'static str {
        match self {
            LogoKind::Png => "png",
            LogoKind::Jpeg => "jpg",
        }
    }
    /// The kind a stored key's extension names.
    pub(crate) fn of_key(key: &str) -> Option<Self> {
        match key.rsplit('.').next()? {
            "png" => Some(LogoKind::Png),
            "jpg" => Some(LogoKind::Jpeg),
            _ => None,
        }
    }
}

/// Width and height of a PNG (its `IHDR`).
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    let ok = b.len() >= 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") && &b[12..16] == b"IHDR";
    let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    ok.then(|| (be(16), be(20)))
}

/// Width and height of a JPEG: the first start-of-frame marker.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return None;
    }
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
        // SOF0..SOF15 but DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u32::from(u16::from_be_bytes([b[i + 5], b[i + 6]]));
            let w = u32::from(u16::from_be_bytes([b[i + 7], b[i + 8]]));
            return Some((w, h));
        }
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

/// The logo's kind, if `bytes` is a PNG or JPEG within the limits.
pub(crate) fn logo(bytes: &[u8]) -> Result<LogoKind, Refused> {
    if bytes.len() > LOGO_MAX_BYTES {
        return Err(Refused::new("kernel.branding.err_logo_large").arg("kb", LOGO_MAX_BYTES / 1024));
    }
    let (kind, size) = match (png_size(bytes), jpeg_size(bytes)) {
        (Some(s), _) => (LogoKind::Png, s),
        (_, Some(s)) => (LogoKind::Jpeg, s),
        _ => return Err(Refused::new("kernel.branding.err_logo_type")),
    };
    let (w, h) = size;
    let fits = |v: u32| (LOGO_MIN_PX..=LOGO_MAX_PX).contains(&v);
    if !fits(w) || !fits(h) {
        return Err(Refused::new("kernel.branding.err_logo_size")
            .arg("w", w)
            .arg("h", h)
            .arg("min", LOGO_MIN_PX)
            .arg("max", LOGO_MAX_PX));
    }
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 6, 0, 0, 0]);
        v
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
        v.extend([0xFF, 0xC0, 0, 11, 8]);
        v.extend(h.to_be_bytes());
        v.extend(w.to_be_bytes());
        v.extend([1, 1, 0x11, 0]);
        v
    }

    #[test]
    fn colours_are_hex_and_readable_under_white_text() {
        assert_eq!(color(" #0B5CAD ").unwrap(), "#0b5cad");
        for bad in ["0b5cad", "#0b5", "#0b5cag", "red", "#0b5cad;x"] {
            assert_eq!(
                color(bad).unwrap_err().key,
                "kernel.branding.err_color",
                "{bad}"
            );
        }
        let e = color("#ffd400").unwrap_err();
        assert_eq!(e.key, "kernel.branding.err_contrast");
        assert!((contrast_with_white([0, 0, 0]) - 21.0).abs() < 0.01);
        assert!((contrast_with_white([255, 255, 255]) - 1.0).abs() < 0.01);
        // #767676 is the classic AA edge: 4.54.
        assert!(color("#767676").is_ok() && color("#777777").is_err());
    }

    #[test]
    fn logos_are_sniffed_and_sized() {
        assert_eq!(logo(&png(200, 80)), Ok(LogoKind::Png));
        assert_eq!(logo(&jpeg(300, 120)), Ok(LogoKind::Jpeg));
        assert_eq!(
            logo(b"<svg/>").unwrap_err().key,
            "kernel.branding.err_logo_type"
        );
        assert_eq!(
            logo(&png(4000, 80)).unwrap_err().key,
            "kernel.branding.err_logo_size"
        );
        let mut big = png(200, 80);
        big.resize(LOGO_MAX_BYTES + 1, 0);
        assert_eq!(
            logo(&big).unwrap_err().key,
            "kernel.branding.err_logo_large"
        );
        assert_eq!(LogoKind::of_key("branding/ab.jpg"), Some(LogoKind::Jpeg));
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(name("  มหาวิทยาลัยหาดแมวน้ำ ").unwrap(), "มหาวิทยาลัยหาดแมวน้ำ");
        assert!(name(" ").is_err() && name(&"ก".repeat(81)).is_err() && name("a\u{7}").is_err());
        let r = name(&"ก".repeat(81)).unwrap_err();
        assert_eq!(r.text(|_| "ไม่เกิน {max} ตัวอักษร".into()), "ไม่เกิน 80 ตัวอักษร");
    }
}
