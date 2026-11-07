//! One SQLite [`Value`] as JSON, so a row survives the trip through text
//! with its storage class intact:
//!
//! | SQLite    | JSON                                  |
//! |-----------|---------------------------------------|
//! | `NULL`    | `null`                                |
//! | `INTEGER` | a number without a fraction: `42`     |
//! | `REAL`    | a number with one: `3.0`, `0.25`      |
//! | `TEXT`    | a string                              |
//! | `BLOB`    | `{"$blob": "<standard base64>"}`      |
//!
//! A non-finite `REAL` (SQLite stores `±Inf`, never `NaN`) is
//! `{"$real": "inf"}` / `{"$real": "-inf"}`, since JSON has no such number.

use serde_json::{Map, Value as Json};

use crate::port_types::Value;
use crate::{Error, Result};

const BLOB: &str = "$blob";
const REAL: &str = "$real";

/// `v` as JSON (see the module docs).
pub(crate) fn encode(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Integer(i) => Json::from(*i),
        Value::Real(f) if f.is_finite() => Json::from(*f),
        Value::Real(f) => tagged(REAL, if *f > 0.0 { "inf" } else { "-inf" }),
        Value::Text(s) => Json::String(s.clone()),
        Value::Blob(b) => tagged(BLOB, &base64_encode(b)),
    }
}

/// The value [`encode`] wrote; anything else is `Error::Invalid`.
pub(crate) fn decode(j: &Json) -> Result<Value> {
    Ok(match j {
        Json::Null => Value::Null,
        // An integer literal is an INTEGER or refused: one beyond i64 is
        // not silently read as a REAL. Reals parse exactly (serde_json's
        // `float_roundtrip`).
        Json::Number(n) if !n.is_f64() => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => return Err(bad(&format!("integer out of range: {n}"))),
        },
        Json::Number(n) => match n.as_f64() {
            Some(f) => Value::Real(f),
            None => return Err(bad(&format!("number out of range: {n}"))),
        },
        Json::String(s) => Value::Text(s.clone()),
        Json::Object(m) if m.len() == 1 => match m.iter().next() {
            Some((k, Json::String(s))) if k == BLOB => Value::Blob(base64_decode(s)?),
            Some((k, Json::String(s))) if k == REAL && s == "inf" => Value::Real(f64::INFINITY),
            Some((k, Json::String(s))) if k == REAL && s == "-inf" => {
                Value::Real(f64::NEG_INFINITY)
            }
            _ => return Err(bad(&format!("unknown tagged value {j}"))),
        },
        other => return Err(bad(&format!("unexpected JSON value {other}"))),
    })
}

fn tagged(tag: &str, s: &str) -> Json {
    let mut m = Map::new();
    m.insert(tag.into(), Json::String(s.into()));
    Json::Object(m)
}

fn bad(m: &str) -> Error {
    Error::Invalid(format!("backup value: {m}"))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding (RFC 4648 §4).
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes [`base64_encode`]'s output; refuses anything malformed.
pub(crate) fn base64_decode(s: &str) -> Result<Vec<u8>> {
    let s = s.as_bytes();
    if !s.len().is_multiple_of(4) {
        return Err(bad("base64 length is not a multiple of 4"));
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for (ci, chunk) in s.chunks(4).enumerate() {
        let last = ci + 1 == s.len() / 4;
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err(bad("misplaced base64 padding"));
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = if i >= 4 - pad {
                0
            } else {
                ALPHABET
                    .iter()
                    .position(|a| a == c)
                    .ok_or_else(|| bad("invalid base64 character"))? as u32
            };
            n = (n << 6) | v;
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(v: Value) -> Value {
        let text = encode(&v).to_string();
        decode(&serde_json::from_str(&text).unwrap()).unwrap()
    }

    #[test]
    fn integers_and_reals_keep_their_storage_class() {
        assert_eq!(encode(&Value::Integer(3)).to_string(), "3");
        assert_eq!(encode(&Value::Real(3.0)).to_string(), "3.0");
        assert_eq!(round_trip(Value::Integer(3)), Value::Integer(3));
        assert_eq!(round_trip(Value::Real(3.0)), Value::Real(3.0));
        assert_eq!(round_trip(Value::Real(0.25)), Value::Real(0.25));
        assert_eq!(
            round_trip(Value::Integer(i64::MIN)),
            Value::Integer(i64::MIN)
        );
        assert_eq!(
            round_trip(Value::Integer(i64::MAX)),
            Value::Integer(i64::MAX)
        );
        assert_eq!(
            round_trip(Value::Real(f64::INFINITY)),
            Value::Real(f64::INFINITY)
        );
        assert_eq!(
            round_trip(Value::Real(f64::NEG_INFINITY)),
            Value::Real(f64::NEG_INFINITY)
        );
    }

    #[test]
    fn reals_round_trip_bit_for_bit() {
        for f in [
            0.1 + 0.2,
            1.234_567_890_123_456_7e-300,
            f64::MIN_POSITIVE,
            f64::MAX,
            -0.0,
            5e-324,
        ] {
            let Value::Real(back) = round_trip(Value::Real(f)) else {
                panic!("{f} came back as another type")
            };
            assert_eq!(back.to_bits(), f.to_bits(), "{f}");
        }
    }

    #[test]
    fn an_integer_beyond_i64_is_refused_not_read_as_a_real() {
        // i64::MAX + 1 and u64::MAX. (serde_json itself reads integers
        // beyond u64 or below i64::MIN as floats; the dump never writes them.)
        for text in ["9223372036854775808", "18446744073709551615"] {
            let j: Json = serde_json::from_str(text).unwrap();
            assert!(decode(&j).is_err(), "{text}");
        }
    }

    #[test]
    fn null_and_text() {
        assert_eq!(encode(&Value::Null), Json::Null);
        assert_eq!(round_trip(Value::Null), Value::Null);
        let thai = Value::Text("ซีลเบย์ \"quoted\"\n{\"$blob\":\"x\"}".into());
        assert_eq!(round_trip(thai.clone()), thai);
    }

    #[test]
    fn blobs_are_tagged_base64() {
        let v = Value::Blob(vec![0, 1, 2, 250, 255]);
        assert_eq!(encode(&v).to_string(), r#"{"$blob":"AAEC+v8="}"#);
        assert_eq!(round_trip(v.clone()), v);
        assert_eq!(round_trip(Value::Blob(vec![])), Value::Blob(vec![]));
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, coded) in cases {
            assert_eq!(base64_encode(plain.as_bytes()), coded);
            assert_eq!(base64_decode(coded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)).unwrap(), all);
    }

    #[test]
    fn malformed_input_is_refused() {
        for s in ["Zg=", "Zg==Zg==", "Z===", "Zm9*"] {
            assert!(base64_decode(s).is_err(), "{s}");
        }
        for j in [
            r#"{"$blob":1}"#,
            r#"{"$other":"x"}"#,
            "[1]",
            r#"{"$real":"nan"}"#,
        ] {
            assert!(decode(&serde_json::from_str(j).unwrap()).is_err(), "{j}");
        }
    }
}
