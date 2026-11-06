//! Cookie headers, a constant-time comparison and the one-shot flash.

use axum::http::{HeaderMap, HeaderValue, header};

use super::FLASH_COOKIE;

/// The value of cookie `name` in the request, if present.
pub(crate) fn get(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// A `Set-Cookie` value scoped to `/admin`. `value` must be cookie-safe
/// (hex tokens, catalog keys). `max_age` 0 deletes the cookie.
pub(crate) fn set(name: &str, value: &str, max_age_secs: i64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    let s = format!(
        "{name}={value}; Path=/admin; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}"
    );
    HeaderValue::from_str(&s).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Deletes cookie `name`.
pub(crate) fn clear(name: &str, secure: bool) -> HeaderValue {
    set(name, "", 0, secure)
}

/// Equal bytes, in time that depends only on the lengths.
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A flash to show on the next page: `ok` and a catalog key.
pub(crate) fn flash(ok: bool, key: &str, secure: bool) -> HeaderValue {
    let v = format!("{}:{key}", if ok { "ok" } else { "err" });
    set(FLASH_COOKIE, &v, 60, secure)
}

/// The pending flash: (`ok`, catalog key). Only keys made of ASCII
/// letters, digits, `.` and `_` are accepted.
pub(crate) fn read_flash(headers: &HeaderMap) -> Option<(bool, String)> {
    let raw = get(headers, FLASH_COOKIE)?;
    let (kind, key) = raw.split_once(':')?;
    let safe = key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_');
    if !safe || key.is_empty() {
        return None;
    }
    Some((kind == "ok", key.to_string()))
}

/// The `_csrf` value of an `application/x-www-form-urlencoded` body. The
/// token is hex, so no percent-decoding is needed.
pub(crate) fn form_field<'a>(body: &'a str, field: &str) -> Option<&'a str> {
    body.split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == field)
        .map(|(_, v)| v)
}

/// The boundary of a `multipart/form-data` content type.
pub(crate) fn multipart_boundary(content_type: &str) -> Option<String> {
    let (kind, params) = content_type.split_once(';')?;
    if !kind.trim().eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    params
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find(|(k, _)| k.eq_ignore_ascii_case("boundary"))
        .map(|(_, v)| v.trim_matches('"').to_string())
        .filter(|b| !b.is_empty())
}

/// The value of the plain (non-file) part `name` of a `multipart/form-data`
/// body: the text between the part's blank line and the next boundary.
pub(crate) fn multipart_field<'a>(body: &'a [u8], boundary: &str, name: &str) -> Option<&'a str> {
    let delim = format!("--{boundary}");
    let header = format!("name=\"{name}\"");
    let mut rest = body;
    while let Some(at) = find(rest, delim.as_bytes()) {
        rest = &rest[at + delim.len()..];
        let end = find(rest, b"\r\n\r\n")?;
        let head = std::str::from_utf8(&rest[..end]).ok()?;
        let value_and_rest = &rest[end + 4..];
        if head.contains(&header) && !head.contains("filename=") {
            let stop = find(value_and_rest, format!("\r\n{delim}").as_bytes())?;
            return std::str::from_utf8(&value_and_rest[..stop]).ok();
        }
    }
    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_found_among_several() {
        let mut h = HeaderMap::new();
        h.append(header::COOKIE, "a=1; hs_session=abc".parse().unwrap());
        h.append(header::COOKIE, "b=2".parse().unwrap());
        assert_eq!(get(&h, "hs_session").as_deref(), Some("abc"));
        assert_eq!(get(&h, "b").as_deref(), Some("2"));
        assert_eq!(get(&h, "c"), None);
    }

    #[test]
    fn multipart_csrf_part() {
        let ct = "multipart/form-data; boundary=\"xyz\"";
        let b = multipart_boundary(ct).unwrap();
        assert_eq!(b, "xyz");
        assert_eq!(multipart_boundary("application/json"), None);
        let body = b"--xyz\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.csv\"\r\n\r\n\
            name=\"_csrf\"\r\n--xyz\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\nabc123\r\n--xyz--\r\n";
        assert_eq!(multipart_field(body, &b, "_csrf"), Some("abc123"));
        assert_eq!(multipart_field(body, &b, "mode"), None);
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn flash_keys_are_restricted() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "hs_flash=ok:kernel.auth.x".parse().unwrap());
        assert_eq!(read_flash(&h), Some((true, "kernel.auth.x".into())));
        h.insert(header::COOKIE, "hs_flash=err:<script>".parse().unwrap());
        assert_eq!(read_flash(&h), None);
    }

    #[test]
    fn set_cookie_attributes() {
        let v = set("hs_session", "t", 60, true);
        assert_eq!(
            v.to_str().unwrap(),
            "hs_session=t; Path=/admin; HttpOnly; SameSite=Lax; Max-Age=60; Secure"
        );
    }
}
