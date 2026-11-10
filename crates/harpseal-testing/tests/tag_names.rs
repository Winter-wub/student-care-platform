//! Tag display names and the Student page's one primary button (#157). A
//! Deployment names a Tag stored as `<name>` (a roster code) with
//! `tag.<name>` in `[texts]`; Staff read that name, the stored name never
//! changes, and the Texts page does not offer it (a saved edit would never
//! show: change it in `[texts]`). The Student page has one primary button.

mod students_support;
mod support;

use harpseal::{BuildError, DeploymentConfig};
use harpseal_testing::{Harness, TestApp};
use students_support::{CONFIG, run, signed_in};

/// The Students Deployment with `scholar` named in Thai.
async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let roles = CONFIG.replace("[roles]\n", "[roles]\nTexts = [\"kernel.setting.edit\"]\n");
    let toml = format!("{roles}\n[texts.th]\n\"tag.scholar\" = \"ทุนการศึกษา\"\n");
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    let app = h.boot(h.builder().config(config).build().unwrap()).await;
    for sql in support::ROSTER {
        run(&h, sql).await;
    }
    run(
        &h,
        "INSERT INTO tag (id, name, created_at) VALUES ('t1', 'scholar', 1), ('t2', 'club', 1)",
    )
    .await;
    run(
        &h,
        "INSERT INTO student_tag (student_id, tag_id, source, added_at) VALUES \
         ('s1', 't1', 'roster', 1), ('s1', 't2', 'manual', 1)",
    )
    .await;
    (h, app)
}

/// The text of every primary button in `main`: a `<button>` or `a.btn`
/// without `secondary` or `link` (the hidden Enter default aside).
fn primaries(html: &str) -> Vec<String> {
    let main = html.split_once("<main").map_or(html, |(_, m)| m);
    let mut out = Vec::new();
    for (open, close) in [("<button", "</button>"), ("<a class=\"btn", "</a>")] {
        for part in main.split(open).skip(1) {
            let tag = part.split_once('>').map_or("", |(t, _)| t);
            let full = format!("{open}{tag}");
            if full.contains("secondary")
                || full.contains("class=\"link")
                || full.contains("sr-default")
            {
                continue;
            }
            let text = part
                .split_once('>')
                .and_then(|(_, r)| r.split_once(close))
                .map_or("", |(t, _)| t);
            out.push(text.trim().to_string());
        }
    }
    out
}

#[tokio::test]
async fn a_tag_reads_its_display_name_and_the_student_page_has_one_primary() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Officer").await;
    let page = b.get("/admin/students/s1").await.body;
    assert!(
        page.contains("<span class=\"chip\">ทุนการศึกษา</span>"),
        "{page}"
    );
    assert!(!page.contains(">scholar<"), "no raw Tag name: {page}");
    assert!(
        page.contains("<span class=\"chip\">club</span>"),
        "unnamed Tags keep their name"
    );
    assert_eq!(primaries(&page), ["แก้ไขข้อมูล"], "{page}");
}

#[tokio::test]
async fn a_tag_name_is_not_offered_on_the_texts_page() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Texts").await;
    assert_eq!(b.get("/admin/texts/tag.scholar").await.status, 404);
    assert!(
        !b.get("/admin/texts")
            .await
            .body
            .contains("/admin/texts/tag.")
    );
}

#[test]
fn an_empty_tag_name_key_is_refused() {
    let toml = format!("{CONFIG}\n[texts.th]\n\"tag.\" = \"ว่าง\"\n");
    let config = DeploymentConfig::from_toml(&toml).unwrap();
    let e = Harness::new().builder().config(config).build().err();
    assert_eq!(
        e,
        Some(BuildError::UnknownText {
            source: "deployment.toml [texts.th]".into(),
            key: "tag.".into(),
        })
    );
}
