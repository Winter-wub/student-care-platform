//! Rich menu names: `harpseal-<key>-<generation>-<hash>` for a Menu with a
//! Menu key, `harpseal-<key>-<generation>-r<revision>-<hash>` for one with
//! a Menu revision too, `harpseal-<hash>` without a key (see
//! [`crate::richmenu`]).

use harpseal::MenuKey;

use super::NAME_PREFIX;

/// Hex digits in a name's content hash.
pub const HASH_LEN: usize = 32;

/// The name of the rich menu with content hash `hash` under Menu key and
/// generation `tag` at Menu revision `revision` (0: none), or without a
/// key (the revision is then left out).
pub(super) fn make(tag: Option<(&MenuKey, u32)>, revision: u32, hash: &str) -> String {
    match tag {
        Some((key, generation)) if revision > 0 => {
            format!(
                "{NAME_PREFIX}{}-{generation}-r{revision}-{hash}",
                key.as_str()
            )
        }
        Some((key, generation)) => {
            format!("{NAME_PREFIX}{}-{generation}-{hash}", key.as_str())
        }
        None => format!("{NAME_PREFIX}{hash}"),
    }
}

/// The Menu key and generation of a rich menu name: `Some(Some((key,
/// generation)))` for `harpseal-<key>-<generation>-<hash>` and
/// `harpseal-<key>-<generation>-r<revision>-<hash>`, `Some(None)` for
/// `harpseal-<hash>`, `None` for any other name (one this adapter did not
/// make, e.g. a hand-made `harpseal-th-backup`). `<hash>` is exactly
/// [`HASH_LEN`] lowercase hex digits.
pub fn name_key(name: &str) -> Option<Option<(&str, u32)>> {
    let rest = name.strip_prefix(NAME_PREFIX)?;
    let parts: Vec<&str> = rest.split('-').collect();
    match parts.as_slice() {
        [hash] if is_hash(hash) => Some(None),
        [key, generation, revision, hash] if is_hash(hash) && is_revision(revision) => {
            Some(Some((valid_key(key)?, generation_of(generation)?)))
        }
        [key, generation, hash] if is_hash(hash) => {
            Some(Some((valid_key(key)?, generation_of(generation)?)))
        }
        _ => None,
    }
}

fn generation_of(generation: &str) -> Option<u32> {
    let digits = !generation.is_empty() && generation.bytes().all(|b| b.is_ascii_digit());
    generation.parse().ok().filter(|_| digits)
}

/// `r` and a revision from 1, without a leading zero.
fn is_revision(s: &str) -> bool {
    s.strip_prefix('r').is_some_and(|n| {
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    }) && s[1..].parse::<u32>().is_ok()
}

fn is_hash(s: &str) -> bool {
    s.len() == HASH_LEN
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A Menu key: [`MenuKey::LOCKED`] or one [`MenuKey::new`] accepts.
fn valid_key(key: &str) -> Option<&str> {
    (key == MenuKey::LOCKED || MenuKey::new(key).is_ok()).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdef0123456789abcdef";

    fn key(k: &str) -> MenuKey {
        MenuKey::new(k).unwrap()
    }

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
            name_key(&format!("harpseal-th-2-r7-{H}")),
            Some(Some(("th", 2)))
        );
        assert_eq!(
            make(Some((&key("th"), 3)), 0, H),
            format!("harpseal-th-3-{H}")
        );
        assert_eq!(
            make(Some((&key("th"), 3)), 12, H),
            format!("harpseal-th-3-r12-{H}")
        );
        assert_eq!(
            make(Some((&MenuKey::locked(), 2)), 0, H),
            format!("harpseal-locked-2-{H}")
        );
        assert_eq!(make(None, 5, H), format!("harpseal-{H}"));
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
            format!("harpseal-th-1-r0-{H}"),
            format!("harpseal-th-1-r01-{H}"),
            format!("harpseal-th-1-7-{H}"),
            format!("harpseal-th-1-r-{H}"),
            "harpseal-0a1b".into(),
            "spring-fair".into(),
        ] {
            assert_eq!(name_key(&name), None, "{name}");
        }
    }
}
