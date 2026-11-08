//! What an uploaded image is, told by its bytes (#109): a PNG or a JPEG
//! only, its name agreeing, and its size in pixels read from its header.
//! No crate: the PNG `IHDR` and the JPEG `SOFn` segment are read here.
//! SVG (script inside an image), GIF, WebP and everything else are
//! refused.

/// An image type the Announcements take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageType {
    Png,
    Jpeg,
}

impl ImageType {
    /// The MIME type served.
    pub(crate) fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }

    /// The extension of the served name.
    pub(crate) fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    /// From a stored content type.
    pub(crate) fn parse(content_type: &str) -> Option<Self> {
        match content_type {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The type of `bytes` and its width and height, when it is a PNG with
/// its `IHDR` or a JPEG with a start-of-frame segment, named as what it is
/// (`.png`; `.jpg` or `.jpeg`; any case). A PNG named `.jpg`, a JPEG
/// without a frame, an SVG, a GIF or text is `None`.
pub(crate) fn sniff(bytes: &[u8], file_name: &str) -> Option<(ImageType, (u32, u32))> {
    let name = file_name.to_ascii_lowercase();
    if bytes.starts_with(PNG_MAGIC) && name.ends_with(".png") {
        return Some((ImageType::Png, png_size(bytes)?));
    }
    let jpeg_name = name.ends_with(".jpg") || name.ends_with(".jpeg");
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) && jpeg_name {
        return Some((ImageType::Jpeg, jpeg_size(bytes)?));
    }
    None
}

/// Longest side an image may have, in pixels.
pub(crate) const MAX_SIDE: u32 = 4096;
/// Most pixels an image may have (16 MP, 4096 x 4096).
pub(crate) const MAX_PIXELS: u64 = 4096 * 4096;
/// Longest side of a Flex hero LINE shows (larger: a warning, #109).
pub(crate) const HERO_SIDE: u32 = 1024;

/// Whether `(w, h)` is within [`MAX_SIDE`] and [`MAX_PIXELS`].
pub(crate) fn size_ok((w, h): (u32, u32)) -> bool {
    w <= MAX_SIDE && h <= MAX_SIDE && u64::from(w) * u64::from(h) <= MAX_PIXELS
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    let x = b.get(at..at + 2)?;
    Some(u32::from(x[0]) << 8 | u32::from(x[1]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let x = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}

/// Width and height from the `IHDR` chunk, which must come first.
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(12..16)? != b"IHDR" {
        return None;
    }
    let (w, h) = (be32(b, 16)?, be32(b, 20)?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Width and height from the first start-of-frame segment. Each step moves
/// forward by a segment's length, so the walk is linear in the file.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut p = 2;
    loop {
        // Fill bytes (0xFF) may precede a marker.
        while b.get(p) == Some(&0xFF) && b.get(p + 1) == Some(&0xFF) {
            p += 1;
        }
        if *b.get(p)? != 0xFF {
            return None;
        }
        let marker = *b.get(p + 1)?;
        match marker {
            // Standalone markers: no length.
            0x01 | 0xD0..=0xD7 => p += 2,
            // End of image or start of scan before any frame: no size.
            0xD9 | 0xDA => return None,
            // SOF0..SOF15 except DHT (C4), JPG (C8), DAC (CC).
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let (h, w) = (be16(b, p + 5)?, be16(b, p + 7)?);
                return (w > 0 && h > 0).then_some((w, h));
            }
            _ => {
                let len = be16(b, p + 2)? as usize;
                if len < 2 {
                    return None;
                }
                p += 2 + len;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG header of `w` x `h` (enough for the sniff; fictional bytes).
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = PNG_MAGIC.to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    /// A JPEG with an APP0 segment, then SOF0 of `w` x `h`.
    pub(crate) fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        b.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        b.extend_from_slice(&[0xFF, 0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        b.extend_from_slice(&[0xFF, 0xD9]);
        b
    }

    #[test]
    fn png_and_jpeg_with_their_sizes() {
        assert_eq!(
            sniff(&png(1040, 676), "map.PNG"),
            Some((ImageType::Png, (1040, 676)))
        );
        assert_eq!(
            sniff(&jpeg(800, 520), "room.jpeg"),
            Some((ImageType::Jpeg, (800, 520)))
        );
        assert_eq!(
            sniff(&jpeg(800, 520), "room.JPG").map(|x| x.0),
            Some(ImageType::Jpeg)
        );
        assert!(size_ok((4096, 4096)) && size_ok((4096, 1)));
        assert!(!size_ok((4097, 10)) && !size_ok((10, 4097)));
    }

    #[test]
    fn everything_else_is_refused() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";
        for (bytes, name) in [
            (png(10, 10), "renamed.jpg"),
            (jpeg(10, 10), "renamed.png"),
            (svg.to_vec(), "logo.svg"),
            (svg.to_vec(), "logo.png"),
            (b"GIF89a\x01\x00\x01\x00".to_vec(), "anim.gif"),
            (b"GIF89a\x01\x00\x01\x00".to_vec(), "anim.png"),
            (b"RIFF\0\0\0\0WEBPVP8 ".to_vec(), "x.jpg"),
            (b"hello".to_vec(), "notes.png"),
            (Vec::new(), "empty.png"),
            // A JPEG with no frame header, a PNG whose IHDR is missing.
            (vec![0xFF, 0xD8, 0xFF, 0xD9], "x.jpg"),
            (b"\x89PNG\r\n\x1a\n\0\0\0\x0dIDAT".to_vec(), "x.png"),
        ] {
            assert_eq!(sniff(&bytes, name), None, "{name}");
        }
    }

    #[test]
    fn a_hostile_jpeg_header_ends_the_walk_and_is_refused() {
        // Zero-length segment, a truncated SOF, a length past the end.
        for b in [
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x00],
            vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00],
            vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF, 1, 2],
            vec![0xFF, 0xD8, 0xFF, 0xFF, 0xFF, 0xFF],
        ] {
            assert_eq!(sniff(&b, "x.jpg"), None);
        }
    }
}
