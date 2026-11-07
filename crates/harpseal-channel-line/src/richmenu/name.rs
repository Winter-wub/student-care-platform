//! Rich menu names: `harpseal-<key>-<generation>-<hash>` for a Menu with a
//! Menu key, `harpseal-<hash>` without one (see [`crate::richmenu`]).

use harpseal::{MenuKey, Result};

use super::NAME_PREFIX;

/// Hex digits in a name's content hash.
pub const HASH_LEN: usize = 32;

/// The name of the rich menu with content hash `hash` under Menu key and
/// generation `tag`, or without a key.
pub(super) fn make(tag: Option<(&str, u32)>, hash: &str) -> Result<String> {
    Ok(match tag {
        Some((key, generation)) => {
            format!("{NAME_PREFIX}{}-{generation}-{hash}", valid_key(key)?)
        }
        None => format!("{NAME_PREFIX}{hash}"),
    })
}

/// The Menu key and generation of a rich menu name: `Some(Some((key,
/// generation)))` for `harpseal-<key>-<generation>-<hash>`, `Some(None)` for
/// `harpseal-<hash>`, `None` for any other name (one this adapter did not
/// make, e.g. a hand-made `harpseal-th-backup`). `<hash>` is exactly
/// [`HASH_LEN`] lowercase hex digits.
pub fn name_key(name: &str) -> Option<Option<(&str, u32)>> {
    let rest = name.strip_prefix(NAME_PREFIX)?;
    let parts: Vec<&str> = rest.split('-').collect();
    match parts.as_slice() {
        [hash] if is_hash(hash) => Some(None),
        [key, generation, hash] if is_hash(hash) => {
            let key = valid_key(key).ok()?;
            let digits = !generation.is_empty() && generation.bytes().all(|b| b.is_ascii_digit());
            let generation: u32 = generation.parse().ok().filter(|_| digits)?;
            Some(Some((key, generation)))
        }
        _ => None,
    }
}

fn is_hash(s: &str) -> bool {
    s.len() == HASH_LEN
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A Menu key, checked as [`MenuKey::new`] does.
fn valid_key(key: &str) -> Result<&str> {
    MenuKey::new(key).map(|_| key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn names_carry_their_menu_key_and_generation() {
        assert_eq!(
            name_key(&format!("harpseal-th-1-{H}")),
            Some(Some(("th", 1)))
        );
        assert_eq!(
            name_key(&format!("harpseal-locked-42-{H}")),
            Some(Some(("locked", 42)))
        );
        assert_eq!(name_key(&format!("harpseal-{H}")), Some(None));
        assert_eq!(
            make(Some(("th", 3)), H).unwrap(),
            format!("harpseal-th-3-{H}")
        );
        assert_eq!(make(None, H).unwrap(), format!("harpseal-{H}"));
    }

    #[test]
    fn hand_made_names_are_not_ours() {
        for name in [
            "harpseal-th-backup".to_string(),
            "harpseal-th-1-backup".into(),
            format!("harpseal-th-{H}"),
            format!("harpseal-Th-1-{H}"),
            format!("harpseal-th-x1-{H}"),
            format!("harpseal-th-+1-{H}"),
            format!("harpseal-th-1-{}", H.to_uppercase()),
            format!("harpseal-th-1-{H}0"),
            "harpseal-0a1b".into(),
            "spring-fair".into(),
        ] {
            assert_eq!(name_key(&name), None, "{name}");
        }
    }
}
