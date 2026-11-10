//! One word per idea on Staff pages (#157). GLOSSARY.md names a Link
//! "การผูกบัญชี", so every Thai default Staff read says ผูก, never the
//! older เชื่อม. The texts a Student reads in the chat or the Mini App
//! (`kernel/texts.rs`, `kernel/texts_link.rs`, `kernel/app/texts.rs`) are
//! not Staff pages and are left out on purpose.

/// Every source file holding Thai defaults that Staff read, by path.
const STAFF_TEXTS: &[(&str, &str)] = &[
    ("auth/texts.rs", include_str!("auth/texts.rs")),
    ("kernel/mod.rs", include_str!("kernel/mod.rs")),
    (
        "kernel/admin/backups/texts.rs",
        include_str!("kernel/admin/backups/texts.rs"),
    ),
    (
        "kernel/admin/consent_review.rs",
        include_str!("kernel/admin/consent_review.rs"),
    ),
    (
        "kernel/admin/module_settings_texts.rs",
        include_str!("kernel/admin/module_settings_texts.rs"),
    ),
    (
        "kernel/admin/texts.rs",
        include_str!("kernel/admin/texts.rs"),
    ),
    (
        "kernel/admin/texts_audit.rs",
        include_str!("kernel/admin/texts_audit.rs"),
    ),
    (
        "kernel/admin/texts_staff.rs",
        include_str!("kernel/admin/texts_staff.rs"),
    ),
    (
        "kernel/branding/texts.rs",
        include_str!("kernel/branding/texts.rs"),
    ),
    ("kernel/home/texts.rs", include_str!("kernel/home/texts.rs")),
    (
        "kernel/roster/texts.rs",
        include_str!("kernel/roster/texts.rs"),
    ),
    (
        "kernel/roster/texts_diff.rs",
        include_str!("kernel/roster/texts_diff.rs"),
    ),
    (
        "kernel/roster/texts_fix.rs",
        include_str!("kernel/roster/texts_fix.rs"),
    ),
    (
        "kernel/roster/texts_reach.rs",
        include_str!("kernel/roster/texts_reach.rs"),
    ),
    (
        "kernel/roster/texts_sheets.rs",
        include_str!("kernel/roster/texts_sheets.rs"),
    ),
    (
        "kernel/roster/texts_warnings.rs",
        include_str!("kernel/roster/texts_warnings.rs"),
    ),
    (
        "kernel/students/texts.rs",
        include_str!("kernel/students/texts.rs"),
    ),
    (
        "kernel/students/texts_bulk.rs",
        include_str!("kernel/students/texts_bulk.rs"),
    ),
    (
        "kernel/usage/texts.rs",
        include_str!("kernel/usage/texts.rs"),
    ),
    (
        "kernel/usage/texts_package.rs",
        include_str!("kernel/usage/texts_package.rs"),
    ),
    (
        "modules/announcements/texts.rs",
        include_str!("modules/announcements/texts.rs"),
    ),
    (
        "modules/announcements/texts_review.rs",
        include_str!("modules/announcements/texts_review.rs"),
    ),
    (
        "modules/announcements/texts_saved.rs",
        include_str!("modules/announcements/texts_saved.rs"),
    ),
    (
        "modules/announcements/page_editor.rs",
        include_str!("modules/announcements/page_editor.rs"),
    ),
    ("ui/onboard_texts.rs", include_str!("ui/onboard_texts.rs")),
    ("ui/picker_texts.rs", include_str!("ui/picker_texts.rs")),
];

/// The verb GLOSSARY.md does not use for a Link.
const OTHER_VERB: &str = "เชื่อม";

#[test]
fn staff_texts_use_one_link_verb() {
    let hits: Vec<String> = STAFF_TEXTS
        .iter()
        .flat_map(|(path, src)| {
            src.lines()
                .enumerate()
                .filter(|(_, l)| l.contains(OTHER_VERB))
                .map(move |(n, l)| format!("{path}:{}: {}", n + 1, l.trim()))
        })
        .collect();
    assert!(hits.is_empty(), "say ผูก for a Link:\n{}", hits.join("\n"));
}

#[test]
fn only_the_final_review_button_says_send() {
    let texts = include_str!("modules/announcements/texts.rs");
    for key in ["announcements.form.confirm", "announcements.action.confirm"] {
        // The key, then its Thai text (on the same line or the next).
        let at = texts
            .find(&format!("\"{key}\""))
            .unwrap_or_else(|| panic!("{key} is a catalog key"));
        let line: String = texts[at..].split_whitespace().take(2).collect();
        assert!(
            line.contains("\"ตรวจก่อนส่ง\""),
            "{key} opens the review: {line}"
        );
    }
    let review = include_str!("modules/announcements/texts_review.rs");
    assert!(review.contains("(\"announcements.review.confirm\", \"ยืนยันส่ง\""));
}
