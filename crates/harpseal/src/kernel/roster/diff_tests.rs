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
        keep: Vec::new(),
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
fn a_stored_null_attribute_is_the_same_as_an_absent_one() {
    let mut stored = fields("นก ทะเล", 1, "active");
    stored.attributes.insert("gpa".into(), Json::Null);
    let e = existing("s1", stored, &[], &[]);
    let r = record(2, "68000001", fields("นก ทะเล", 1, "active"), &[]);
    let (t, _) = classify(Some(&e), &r);
    assert_eq!((t.unchanged, t.updated), (1, 0));
    assert_eq!(change_of(Some(&e), &r), None);
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
    assert!(
        p.left_names.is_empty(),
        "the leavers are in the head, not counts"
    );
    let head = Head::of(&all);
    assert_eq!(head.leaving.len(), SHOWN_LEAVING);
    assert_eq!(head.leaving[0].code, "68000001", "by code");
    assert_eq!(head.leaving[0].links, 2);
    assert_eq!(head.changes.len(), 1);
    assert_eq!(head.changes[0].fields[0].field, "attributes.year");
    assert_eq!(all.len(), 250, "every change is stored");
    assert_eq!(all.iter().filter(|c| c.kind == "leaving").count(), 249);
    // The counts carry no Student (#134).
    let counts = serde_json::to_string(&p).unwrap();
    assert!(
        !counts.contains("คน 1") && !counts.contains("68000001"),
        "{counts}"
    );
    // Upsert makes nobody left.
    let (p, all) = preview(&valid, &students, false, &HashMap::new());
    assert!(p.links_cut == 0 && Head::of(&all).leaving.is_empty());
}

#[test]
fn the_head_cuts_long_values_and_the_list_keeps_them() {
    let long = "ก".repeat(SHOWN_CHARS + 50);
    let c = Change {
        code: "68000001".into(),
        name: "นก".into(),
        kind: "updated".into(),
        fields: vec![FieldChange {
            field: "attributes.note".into(),
            before: json!(long),
            after: json!(["สั้น", long]),
        }],
        links: 0,
    };
    let head = Head::of(std::slice::from_ref(&c));
    let cut = format!("{}…", "ก".repeat(SHOWN_CHARS));
    assert_eq!(head.changes[0].fields[0].before, json!(cut));
    assert_eq!(head.changes[0].fields[0].after, json!(["สั้น", cut]));
    assert_eq!(
        decode(&encode(std::slice::from_ref(&c)).unwrap())[0],
        c,
        "the CSV's copy is whole"
    );
    assert_eq!(head_key("abc"), "roster-diff/abc.head.json");
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

#[test]
fn a_column_left_out_keeps_the_stored_value() {
    // #154: the file has no email, Tags or year: nothing changes.
    use crate::kernel::roster::validate::resolve;
    let e = existing("s1", fields("นก ทะเล", 3, "active"), &[], &["ชมรม"]);
    let mut f = fields("นก ทะเล", 3, "active");
    f.email = None;
    f.attributes.clear();
    let mut r = record(2, "68000001", f, &[]);
    r.keep = ["email", "tags", "attributes.year"]
        .map(String::from)
        .to_vec();
    let kept = resolve(Some(&e), &r);
    assert_eq!(change_of(Some(&e), &kept), None);
    // Without the keep list the same record clears them.
    r.keep.clear();
    let c = change_of(Some(&e), &resolve(Some(&e), &r)).unwrap();
    assert_eq!(c.fields.len(), 3, "{c:?}");
    // A new Student keeps the record's empty values.
    assert!(resolve(None, &r).fields.email.is_none());
}
