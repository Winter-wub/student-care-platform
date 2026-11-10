//! Ids and codes never wrap inside the token (#140): the cells and lines
//! that hold a student id carry `.id`, which the stylesheet keeps on one
//! line, and a chip breaks between words only. Checked at 360 px in a
//! browser for the PR; here, the markup and the rules it relies on.

mod roster_support;

use roster_support::*;

/// The declarations of the stylesheet's last rule for `selector` (the one that wins).
fn rule(selector: &str) -> String {
    let css = harpseal::ui::CSS;
    let at = css
        .rfind(&format!("\n{selector} {{"))
        .unwrap_or_else(|| panic!("no rule {selector}"));
    let body = &css[at..];
    body[..body.find('}').unwrap()].to_string()
}

#[test]
fn the_stylesheet_keeps_an_id_whole_and_lets_a_chip_break_only_between_words() {
    let id = rule(".id");
    assert!(id.contains("white-space: nowrap"), "{id}");
    assert!(id.contains("overflow-wrap: normal"), "{id}");
    let chip = rule(".chip");
    assert!(chip.contains("overflow-wrap: break-word"), "{chip}");
    // Help lines read at a measure, not the page's width.
    assert!(rule("p.muted, .muted p").contains("max-width: 60ch"));
}

#[tokio::test]
async fn student_ids_are_marked_whole_on_the_list_the_detail_and_an_import() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "Officer").await;
    let id = previewed(&b, "roster.csv", CSV_50, "full", false).await;
    assert_eq!(confirm(&b, &id).await.status, 303);
    run_jobs(&h, &app).await;
    let list = b.get("/admin/students").await.body;
    let cells = list.matches("<td class=\"code id\"").count();
    assert!(cells >= 20, "{cells} id cells");
    // The first Student's page names the id whole.
    let student = list
        .split("href=\"/admin/students/")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("a Student link");
    let detail = b.get(&format!("/admin/students/{student}")).await.body;
    assert!(detail.contains("<p class=\"code id\">"), "{detail}");
    assert_eq!(
        harpseal_testing::a11y::problems(&detail),
        Vec::<String>::new()
    );
}
