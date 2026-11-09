//! The Audience form shows enum labels (#81) while the filter keeps the
//! stored value: the chips, the posted values and the summary on the list.

mod announcements_support;

use announcements_support::*;

const SRC: &str = r#"
name = "Seal Bay University (audience labels)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["kernel.student.view", "announcements.view", "announcements.send"]

[roster.attributes.status]
column = "สถานะ"
type = "enum"
values = ["current", "graduated"]
value_labels = { current = { th = "กำลังศึกษา", en = "Studying" }, graduated = { th = "จบการศึกษา" } }
label = { th = "สถานะ", en = "Status" }
"#;

#[tokio::test]
async fn the_audience_form_shows_labels_and_posts_the_stored_value() {
    let w = boot_config(SRC, 500).await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    assert!(
        page.contains(r#"name="attr.status" value="current""#) && page.contains("กำลังศึกษา"),
        "{page}"
    );
    assert!(
        page.contains(r#"value="graduated""#) && page.contains("จบการศึกษา"),
        "{page}"
    );
    assert!(
        !page.contains("> current<"),
        "the raw value is not a chip: {page}"
    );

    // The Audience filters on the value, and the list shows the label.
    let id = create(&b, &form("ก", "ข", "&attr.status=graduated")).await;
    let stored = texts(&w, "SELECT audience FROM announcement").await;
    assert!(stored[0].contains("graduated"), "{stored:?}");
    let list = b.get("/admin/announcements").await.body;
    // The attribute reads by its label too (#125), not its key.
    assert!(list.contains("สถานะ: จบการศึกษา"), "{list}");
    assert!(!list.contains("status:"), "{list}");
    let detail = b.get(&format!("/admin/announcements/{id}")).await.body;
    assert!(detail.contains("สถานะ: จบการศึกษา"), "{detail}");
}
