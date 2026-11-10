//! Unit tests of [`super`]: the legacy split rules and the seed's decision.

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

fn legacy(p: &[&str]) -> Vec<String> {
    with_legacy_grants(&ids(p)).0
}

#[test]
fn a_role_listing_only_the_old_link_permission_gets_both() {
    let got = legacy(&["kernel.student.view", LINK_MANAGE]);
    assert_eq!(got, ids(&["kernel.student.view", LINK_MANAGE, LINK_REVOKE]));
}

#[test]
fn a_role_listing_only_the_setting_permission_also_edits_texts() {
    let (got, widened) = with_legacy_grants(&ids(&[SETTING_EDIT]));
    assert_eq!(got, ids(&[SETTING_EDIT, TEXTS_EDIT]));
    let pairs: Vec<_> = widened.iter().map(|s| (s.old, s.new)).collect();
    assert_eq!(pairs, [(SETTING_EDIT, TEXTS_EDIT)]);
    let (got, widened) = with_legacy_grants(&ids(&[LINK_MANAGE, SETTING_EDIT]));
    assert_eq!(
        got,
        ids(&[LINK_MANAGE, SETTING_EDIT, LINK_REVOKE, TEXTS_EDIT])
    );
    assert_eq!(widened.len(), 2);
}

#[test]
fn a_role_listing_the_new_permission_is_taken_as_written() {
    for written in [
        &[LINK_REVOKE][..],
        &[LINK_MANAGE, LINK_REVOKE],
        &["kernel.student.view"],
        &[TEXTS_EDIT],
        &[SETTING_EDIT, TEXTS_EDIT],
    ] {
        assert_eq!(legacy(written), ids(written), "{written:?}");
        assert!(with_legacy_grants(&ids(written)).1.is_empty());
    }
}

/// The plan for an existing TOML Role listing only `kernel.setting.edit`,
/// seeded before #165 (the Role and the old pair recorded), holding `held`.
fn texts_split(held: &[&str]) -> Step {
    let s = seen(&[("Editor", ""), ("Editor", SETTING_EDIT)]);
    let (wanted, widened) = with_legacy_grants(&ids(&[SETTING_EDIT]));
    let held = holding(held);
    let mut step = plan("Editor", &wanted, &s, Some(&held));
    drop_orphan_splits(&mut step, &widened, Some(&held));
    step
}

#[test]
fn a_split_never_grants_the_new_permission_once_the_old_was_taken_away() {
    let step = texts_split(&["kernel.audit.view"]);
    assert_eq!(step.record, ids(&[TEXTS_EDIT]), "recorded as seeded");
    assert!(step.grant.is_empty(), "not granted: {step:?}");
    let step = texts_split(&[SETTING_EDIT]);
    assert_eq!(step.grant, ids(&[TEXTS_EDIT]), "the old one is held");
    let step = texts_split(&[SETTING_EDIT, TEXTS_EDIT]);
    assert!(step.grant.is_empty() && step.record == ids(&[TEXTS_EDIT]));
}

#[test]
fn a_split_grants_the_new_permission_with_the_old_one_in_the_same_step() {
    let (wanted, widened) = with_legacy_grants(&ids(&[SETTING_EDIT]));
    let held = holding(&["kernel.audit.view"]);
    let mut step = plan("Editor", &wanted, &seen(&[("Editor", "")]), Some(&held));
    drop_orphan_splits(&mut step, &widened, Some(&held));
    assert_eq!(step.grant, ids(&[SETTING_EDIT, TEXTS_EDIT]));
    let mut created = plan("Editor", &wanted, &seen(&[]), None);
    drop_orphan_splits(&mut created, &widened, None);
    assert_eq!(created.grant, ids(&[SETTING_EDIT, TEXTS_EDIT]));
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
