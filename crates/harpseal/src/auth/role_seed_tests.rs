//! Unit tests of [`super`]: the legacy link rule and the seed's decision.

use super::*;

fn ids(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

fn seen(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    pairs
        .iter()
        .map(|(r, p)| (r.to_string(), p.to_string()))
        .collect()
}

fn holding(p: &[&str]) -> Held {
    Held {
        id: "r1".into(),
        permissions: p.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn a_role_listing_only_the_old_link_permission_gets_both() {
    let got = with_legacy_link_grant(&ids(&["kernel.student.view", LINK_MANAGE]));
    assert_eq!(got, ids(&["kernel.student.view", LINK_MANAGE, LINK_REVOKE]));
}

#[test]
fn a_role_listing_the_new_permission_is_taken_as_written() {
    let only_revoke = ids(&[LINK_REVOKE]);
    assert_eq!(with_legacy_link_grant(&only_revoke), only_revoke);
    let both = ids(&[LINK_MANAGE, LINK_REVOKE]);
    assert_eq!(with_legacy_link_grant(&both), both);
    let neither = ids(&["kernel.student.view"]);
    assert_eq!(with_legacy_link_grant(&neither), neither);
}

#[test]
fn a_new_role_is_created_with_every_pair() {
    let step = plan("Reader", &ids(&["b", "a"]), &seen(&[]), None);
    assert!(step.create && step.record_role);
    assert_eq!(step.record, ids(&["a", "b"]));
    assert_eq!(step.grant, ids(&["a", "b"]));
}

#[test]
fn a_seeded_pair_taken_away_is_not_granted_again() {
    let s = seen(&[("Reader", ""), ("Reader", "a"), ("Reader", "b")]);
    let step = plan("Reader", &ids(&["a", "b"]), &s, Some(&holding(&["b"])));
    assert_eq!(step, Step::default());
}

#[test]
fn only_the_new_pair_is_granted() {
    let s = seen(&[("Reader", ""), ("Reader", "a")]);
    let step = plan("Reader", &ids(&["a", "c"]), &s, Some(&holding(&[])));
    assert!(!step.create && !step.record_role);
    assert_eq!(step.record, ids(&["c"]));
    assert_eq!(step.grant, ids(&["c"]));
}

#[test]
fn an_existing_database_records_what_it_holds_and_grants_the_rest() {
    let step = plan(
        "Reader",
        &ids(&["a", "b"]),
        &seen(&[]),
        Some(&holding(&["a"])),
    );
    assert!(!step.create && step.record_role);
    assert_eq!(step.record, ids(&["a", "b"]));
    assert_eq!(
        step.grant,
        ids(&["b"]),
        "only what the old migrate added too"
    );
}

#[test]
fn a_deleted_seeded_role_is_left_deleted_but_its_new_pairs_are_recorded() {
    let s = seen(&[("Reader", ""), ("Reader", "a")]);
    let step = plan("Reader", &ids(&["a", "new"]), &s, None);
    let recorded = Step {
        record: ids(&["new"]),
        ..Step::default()
    };
    assert_eq!(step, recorded);
}
