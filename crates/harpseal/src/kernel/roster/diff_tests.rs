use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::kernel::roster::preview::preview;
use crate::kernel::roster::store::{Existing, classify};
use crate::kernel::roster::validate::{Record, Valid};
use crate::message::Locale;
use crate::students::StudentFields;

fn fields(name: &str, year: i64, status: &str) -> StudentFields {
    let mut f = StudentFields::new(
        name,
        Some("nok@student.sealbay.example".into()),
        Locale("th".into()),
    )
    .with_status(status);
    f.attributes.insert("year".into(), json!(year));
    f
}

fn existing(id: &str, f: StudentFields, overrides: &[&str], tags: &[&str]) -> Existing {
    Existing {
        id: id.into(),
        fields: f,
        overrides: overrides.iter().map(|o| o.to_string()).collect(),
        raw_overrides: String::new(),
        roster_tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

fn record(row: usize, code: &str, f: StudentFields, tags: &[&str]) -> Record {
    Record {
        row,
        code: code.into(),
        fields: f,
        tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

fn change_of(e: Option<&Existing>, r: &Record) -> Option<Change> {
    let (t, m) = classify(e, r);
    change(e, r, &t, &m)
}

#[test]
fn a_new_student_is_new_with_no_fields() {
    let r = record(2, "68000001", fields("นก ทะเล", 1, "active"), &[]);
    let c = change_of(None, &r).unwrap();
    assert_eq!((c.kind.as_str(), c.name.as_str()), ("new", "นก ทะเล"));
    assert!(c.fields.is_empty());
}

#[test]
fn an_update_lists_each_changed_field_old_to_new() {
    let e = existing("s1", fields("นก ทะเล", 1, "active"), &[], &["ชมรม"]);
    let mut f = fields("นก ทะเล", 2, "active");
    f.email = Some("nok@student.sealbay.example".into());
    let r = record(2, "68000001", f, &["ทุน", "ชมรม"]);
    let c = change_of(Some(&e), &r).unwrap();
    assert_eq!(c.kind, "updated");
    let got: Vec<(&str, &Json, &Json)> = c
        .fields
        .iter()
        .map(|f| (f.field.as_str(), &f.before, &f.after))
        .collect();
    assert_eq!(
        got,
        vec![
            ("attributes.year", &json!(1), &json!(2)),
            ("tags", &json!(["ชมรม"]), &json!(["ชมรม", "ทุน"])),
        ]
    );
}

#[test]
fn a_field_staff_hold_is_not_a_change_and_an_unchanged_row_is_none() {
    let e = existing(
        "s1",
        fields("นก ทะเล", 1, "active"),
        &["attributes.year"],
        &[],
    );
    let r = record(2, "68000001", fields("นก ทะเล", 2, "active"), &[]);
    assert_eq!(change_of(Some(&e), &r), None, "the held year stays");
    let same = record(2, "68000001", fields("นก ทะเล", 1, "active"), &[]);
    assert_eq!(change_of(Some(&e), &same), None);
}

#[test]
fn a_returning_student_shows_the_status_change() {
    let e = existing("s1", fields("นก ทะเล", 1, "left"), &[], &[]);
    let r = record(2, "68000001", fields("นก ทะเล", 1, "active"), &[]);
    let c = change_of(Some(&e), &r).unwrap();
    assert_eq!(c.kind, "returning");
    assert_eq!(c.fields[0].field, "status");
    assert_eq!(
        (&c.fields[0].before, &c.fields[0].after),
        (&json!("left"), &json!("active"))
    );
}

#[test]
fn the_preview_lists_leavers_by_code_with_their_links_and_caps_the_lists() {
    let mut students = HashMap::new();
    for n in 0..250u64 {
        let id = format!("s{n}");
        students.insert(
            format!("6800{n:04}"),
            existing(&id, fields(&format!("คน {n}"), 1, "active"), &[], &[]),
        );
    }
    // One held status stays; one listed Student changes year.
    students.insert(
        "69000000".into(),
        existing("held", fields("คงไว้", 1, "active"), &["status"], &[]),
    );
    let listed = record(2, "68000000", fields("คน 0", 2, "active"), &[]);
    let valid = Valid {
        records: vec![listed],
        empty_email: 0,
        warnings: Vec::new(),
    };
    let links = HashMap::from([
        ("s1".to_string(), 2),
        ("s2".to_string(), 1),
        ("held".to_string(), 5),
    ]);
    let (p, all) = preview(&valid, &students, true, &links);
    assert!(p.detailed);
    assert_eq!(p.will_become_left, 249);
    assert_eq!(p.kept_by_status, 1);
    assert_eq!(p.links_cut, 3, "the held Student keeps their Links");
    assert_eq!(p.leaving.len(), SHOWN_LEAVING);
    assert_eq!(p.leaving[0].code, "68000001", "by code");
    assert_eq!(p.leaving[0].links, 2);
    assert_eq!(p.changes.len(), 1);
    assert_eq!(p.changes[0].fields[0].field, "attributes.year");
    assert_eq!(all.len(), 250, "every change is stored");
    assert_eq!(all.iter().filter(|c| c.kind == "leaving").count(), 249);
    // Upsert makes nobody left.
    let (p, _) = preview(&valid, &students, false, &HashMap::new());
    assert!(p.leaving.is_empty() && p.links_cut == 0);
}

#[test]
fn the_stored_list_reads_back_and_skips_a_broken_line() {
    let c = Change {
        code: "68000001".into(),
        name: "นก".into(),
        kind: "leaving".into(),
        fields: Vec::new(),
        links: 2,
    };
    let mut bytes = encode(std::slice::from_ref(&c)).unwrap();
    bytes.extend_from_slice(b"{broken\n");
    assert_eq!(decode(&bytes), vec![c]);
    assert_eq!(key("abc"), "roster-diff/abc.jsonl");
}
