//! Telegram Web App (`Mini App`) `initData` validation
//! (<https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app>):
//! the data-check-string is every field but `hash`, sorted by key, as
//! `key=value` lines joined by `\n`; `secret_key = HMAC-SHA256(key =
//! "WebAppData", msg = bot_token)`; `hash` must equal the hex
//! `HMAC-SHA256(key = secret_key, msg = data_check_string)`; `auth_date`
//! must be recent.

use harpseal::{ChannelAccountId, Error, Result, Timestamp};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::KIND;
use crate::update::constant_time_eq;

/// How old `initData` may be, in seconds. A Mini App reads it once when it
/// opens and exchanges it for an App Session, so an hour is plenty.
pub const INIT_DATA_MAX_AGE_SECS: i64 = 3600;
/// Clock skew allowed for an `auth_date` slightly in the future.
const FUTURE_SKEW_SECS: i64 = 60;

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decodes one `application/x-www-form-urlencoded` component (`+` is a
/// space, `%XX` a byte). `None` on a broken escape or invalid UTF-8.
fn form_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let h = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn invalid(what: &str) -> Error {
    Error::Invalid(format!("Telegram initData: {what}"))
}

/// Checks `init_data` (the raw `Telegram.WebApp.initData` query string)
/// for `bot_token` at `now`, and returns the account of its `user`.
/// A wrong hash is [`Error::BadSignature`]; a malformed or stale one is
/// [`Error::Invalid`].
pub fn verify_init_data(
    init_data: &str,
    bot_token: &str,
    now: Timestamp,
) -> Result<ChannelAccountId> {
    let mut fields = Vec::new();
    let mut hash = None;
    for pair in init_data.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| invalid("a field without `=`"))?;
        let (k, v) = (
            form_decode(k).ok_or_else(|| invalid("bad escape"))?,
            form_decode(v).ok_or_else(|| invalid("bad escape"))?,
        );
        if k == "hash" {
            hash = Some(v);
        } else {
            fields.push((k, v));
        }
    }
    let hash = hash.ok_or(Error::BadSignature)?;
    fields.sort();
    let check: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let secret = hmac_sha256(b"WebAppData", bot_token.as_bytes());
    let want = hex(&hmac_sha256(&secret, check.join("\n").as_bytes()));
    if !constant_time_eq(want.as_bytes(), hash.to_ascii_lowercase().as_bytes()) {
        return Err(Error::BadSignature);
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let auth_date: i64 = field("auth_date")
        .and_then(|d| d.parse().ok())
        .ok_or_else(|| invalid("no auth_date"))?;
    let age = now.ms() / 1000 - auth_date;
    if !(-FUTURE_SKEW_SECS..=INIT_DATA_MAX_AGE_SECS).contains(&age) {
        return Err(invalid("auth_date is not recent"));
    }
    let user: serde_json::Value = field("user")
        .and_then(|u| serde_json::from_str(u).ok())
        .ok_or_else(|| invalid("no user"))?;
    let id = user["id"].as_i64().ok_or_else(|| invalid("no user id"))?;
    // A Student's private chat with the bot has the user's id.
    Ok(ChannelAccountId::new(KIND, id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Derived outside this code with Python's `hmac`/`hashlib`:
    ///
    /// ```text
    /// secret = hmac.new(b"WebAppData", TOKEN.encode(), sha256).digest()
    /// dcs = "auth_date=1793836800\nquery_id=AAHdF6IQAAAAAN0XohDhrOrc\n"
    ///       'user={"id":7012345678,"first_name":"Karn","language_code":"th"}'
    /// hmac.new(secret, dcs.encode(), sha256).hexdigest()
    /// == "2e9f10864e5d25e3ed0571951dab659b75948576b076b8d92fc2b059b003b1b1"
    /// ```
    const TOKEN: &str = "8000000001:AAFsealBayTestTokenNotReal0000000000";
    const INIT_DATA: &str = "auth_date=1793836800&query_id=AAHdF6IQAAAAAN0XohDhrOrc\
        &user=%7B%22id%22%3A7012345678%2C%22first_name%22%3A%22Karn%22%2C%22language_code%22%3A%22th%22%7D\
        &hash=2e9f10864e5d25e3ed0571951dab659b75948576b076b8d92fc2b059b003b1b1";
    /// 2026-11-05T00:00:00Z plus five minutes.
    const NOW: Timestamp = Timestamp((1_793_836_800 + 300) * 1000);

    #[test]
    fn the_known_vector_verifies() {
        let who = verify_init_data(INIT_DATA, TOKEN, NOW).unwrap();
        assert_eq!(who, ChannelAccountId::new(KIND, "7012345678"));
    }

    #[test]
    fn field_order_does_not_matter() {
        let (head, hash) = INIT_DATA.rsplit_once('&').unwrap();
        let mut parts: Vec<&str> = head.split('&').collect();
        parts.reverse();
        let shuffled = format!("{hash}&{}", parts.join("&"));
        assert!(verify_init_data(&shuffled, TOKEN, NOW).is_ok());
    }

    #[test]
    fn tampering_and_wrong_tokens_are_refused() {
        let tampered = INIT_DATA.replace("7012345678", "7012345679");
        assert!(matches!(
            verify_init_data(&tampered, TOKEN, NOW),
            Err(Error::BadSignature)
        ));
        let other = "8000000002:AAFsealBayTestTokenNotReal0000000000";
        assert!(matches!(
            verify_init_data(INIT_DATA, other, NOW),
            Err(Error::BadSignature)
        ));
        let no_hash = INIT_DATA.rsplit_once('&').unwrap().0;
        assert!(matches!(
            verify_init_data(no_hash, TOKEN, NOW),
            Err(Error::BadSignature)
        ));
    }

    #[test]
    fn stale_or_future_auth_date_is_refused() {
        let late = Timestamp(NOW.ms() + INIT_DATA_MAX_AGE_SECS * 1000);
        assert!(matches!(
            verify_init_data(INIT_DATA, TOKEN, late),
            Err(Error::Invalid(_))
        ));
        let early = Timestamp((1_793_836_800 - 120) * 1000);
        assert!(matches!(
            verify_init_data(INIT_DATA, TOKEN, early),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn form_decoding() {
        assert_eq!(form_decode("a+b%20c").as_deref(), Some("a b c"));
        assert_eq!(form_decode("%E0%B8%81").as_deref(), Some("ก"));
        assert_eq!(form_decode("%G0"), None);
        assert_eq!(form_decode("%2"), None);
    }
}
