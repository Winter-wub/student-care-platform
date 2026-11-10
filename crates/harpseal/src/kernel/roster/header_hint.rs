//! Near-match hints for a header the roster does not know (#154): the
//! header error suggests the expected header a found one is close to. A
//! header is close when it reads the same once case, white space and
//! `_ - . : *` are ignored; when it is the column's label (Thai or
//! English, as the help names it, or an attribute's key); or when it is
//! one or two letters off (a typo). Only headers the sheet still lacks
//! are suggested. Pure: the names come in as arguments.

use crate::config::RosterConfig;
use crate::manifest::Catalog;
use crate::message::Locale;

/// One expected header and the other names Staff may give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Names {
    pub(super) header: String,
    /// Normalised ([`norm`]): the header itself, its labels, its key.
    names: Vec<String>,
}

/// `s` as headers are compared: lower case, without white space,
/// `_ - . : *` or invisible characters.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '_' | '-' | '.' | ':' | '*'))
        .filter(|c| !matches!(c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}'))
        .flat_map(char::to_lowercase)
        .collect()
}

impl Names {
    /// `header` known also by `others` (labels, a key).
    pub(super) fn new<'a>(header: &str, others: impl IntoIterator<Item = &'a str>) -> Self {
        let mut names = vec![norm(header)];
        for o in others {
            // A label with its notes: `อีเมล (ใช้ติดต่อ)` is `อีเมล`.
            let o = norm(o.split(" (").next().unwrap_or(o));
            if !o.is_empty() && !names.contains(&o) {
                names.push(o);
            }
        }
        Self {
            header: header.to_string(),
            names,
        }
    }
}

/// Every `[roster]` column with its labels in Thai and English (the
/// catalog's for the Kernel's columns, the config's for attributes).
pub(super) fn names(catalog: &Catalog, roster: &RosterConfig) -> Vec<Names> {
    let labels = |key: &str| -> Vec<String> {
        ["th", "en"]
            .iter()
            .map(|l| catalog.t(&Locale((*l).into()), key))
            .collect()
    };
    let mut out = Vec::new();
    let mut core = vec![
        (roster.student_code.as_str(), "kernel.students.code"),
        (roster.display_name.as_str(), "kernel.students.name"),
        (roster.email.as_str(), "kernel.students.email"),
    ];
    core.extend(
        roster
            .locale
            .as_ref()
            .map(|l| (l.column.as_str(), "kernel.students.locale")),
    );
    core.extend(
        roster
            .tags
            .as_ref()
            .map(|t| (t.column.as_str(), "kernel.students.tags")),
    );
    for (header, key) in core {
        let l = labels(key);
        out.push(Names::new(header, l.iter().map(String::as_str)));
    }
    for (key, a) in roster.attributes_in_order() {
        let others = std::iter::once(key).chain(a.label.values().map(String::as_str));
        out.push(Names::new(&a.column, others));
    }
    out
}

/// The expected header `found` is likely meant to be, among those not in
/// `present`; `None` when nothing is close.
pub(super) fn near<'a>(found: &str, columns: &'a [Names], present: &[String]) -> Option<&'a str> {
    let f = norm(found);
    if f.is_empty() {
        return None;
    }
    // A known header (a repeated one) is not "close to" itself.
    let open = || {
        columns
            .iter()
            .filter(|c| c.header != found.trim() && !present.contains(&c.header))
    };
    if let Some(c) = open().find(|c| c.names.contains(&f)) {
        return Some(&c.header);
    }
    // A typo: one letter off in a short name, two in a longer one.
    let most = |n: usize| if n <= 4 { 1 } else { 2 };
    open()
        .filter_map(|c| {
            let d = c
                .names
                .iter()
                .map(|n| (distance(&f, n), n.chars().count()))
                .filter(|(d, len)| *d <= most((*len).min(f.chars().count())))
                .map(|(d, _)| d)
                .min()?;
            Some((d, c.header.as_str()))
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, h)| h)
}

/// The edit distance of `a` and `b`, by characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<Names> {
        vec![
            Names::new("รหัสนักศึกษา", ["รหัสนักศึกษา", "Student id"]),
            Names::new("อีเมล", ["อีเมล", "Email"]),
            Names::new("เกรดเฉลี่ย", ["gpa", "GPA"]),
        ]
    }

    #[test]
    fn case_spaces_and_labels_find_the_header() {
        let c = columns();
        assert_eq!(near("EMAIL", &c, &[]), Some("อีเมล"));
        assert_eq!(near(" e-mail ", &c, &[]), Some("อีเมล"));
        assert_eq!(near("รหัส นักศึกษา", &c, &[]), Some("รหัสนักศึกษา"));
        assert_eq!(near("student_id", &c, &[]), Some("รหัสนักศึกษา"));
        assert_eq!(near("GPA", &c, &[]), Some("เกรดเฉลี่ย"));
    }

    #[test]
    fn a_typo_is_close_and_a_stranger_is_not() {
        let c = columns();
        assert_eq!(near("อีเมลล์", &c, &[]), Some("อีเมล"));
        assert_eq!(near("รหัสนักศึกษ", &c, &[]), Some("รหัสนักศึกษา"));
        assert_eq!(near("หมายเหตุ", &c, &[]), None);
        assert_eq!(near("", &c, &[]), None);
    }

    #[test]
    fn a_header_already_in_the_sheet_is_not_suggested() {
        let c = columns();
        assert_eq!(near("Email", &c, &["อีเมล".to_string()]), None);
    }

    #[test]
    fn distance_counts_characters() {
        assert_eq!(distance("อีเมล", "อีเมลล์"), 2);
        assert_eq!(distance("abc", "abc"), 0);
        assert_eq!(distance("", "ab"), 2);
    }
}
