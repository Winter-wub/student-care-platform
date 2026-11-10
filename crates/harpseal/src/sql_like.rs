//! `LIKE` patterns from what Staff typed, shared by every search that uses
//! one (the Students list, the admin filters, the Student picker). Use the
//! pattern with `LIKE ? ESCAPE '\'`.

/// Longest `LIKE` pattern in bytes: Cloudflare D1 refuses more than 50.
pub(crate) const MAX_LIKE: usize = 50;

/// `q` as a `LIKE` pattern (`\` escapes `%`, `_` and `\`): with `anywhere`,
/// `%` on both sides, else only after (a prefix match). It is cut after the
/// last whole character that keeps the pattern, both `%` and the escapes
/// included, within [`MAX_LIKE`] bytes.
pub(crate) fn like(q: &str, anywhere: bool) -> String {
    let mut p = String::from(if anywhere { "%" } else { "" });
    for c in q.chars() {
        let escape = matches!(c, '%' | '_' | '\\');
        if p.len() + usize::from(escape) + c.len_utf8() + 1 > MAX_LIKE {
            break;
        }
        if escape {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

#[cfg(test)]
mod tests {
    use super::like;

    #[test]
    fn wildcards_are_escaped() {
        assert_eq!(like("a%b_c\\", true), "%a\\%b\\_c\\\\%");
        assert_eq!(like("", true), "%%");
        assert_eq!(like("student.", false), "student.%");
        assert_eq!(like("6801", false), "6801%");
    }

    #[test]
    fn patterns_fit_the_d1_limit_on_whole_characters() {
        // Thai letters are 3 bytes each: 16 fit (1 + 48 + 1 = 50), the
        // 17th would make 53.
        let p = like(&"ก".repeat(20), true);
        assert_eq!(p.len(), 50);
        assert_eq!(p.chars().filter(|c| *c == 'ก').count(), 16);
        // Escapes count, and are never split from their character.
        let p = like(&"%".repeat(40), true);
        assert!(p.len() <= 50 && p.ends_with("\\%%"), "{p}");
        assert_eq!(like(&"a".repeat(80), true).len(), 50);
        assert_eq!(like(&"a".repeat(80), false).len(), 50);
    }
}
