//! Menu variants (#182) at the Deployment seam: a graduate's Menu, Menu
//! Choices and Mini App home keep only what `[[menu.variants]]` lists; a
//! Staff edit or a roster import that changes the status re-links the
//! Menu; a Menu label save re-links both kinds of accounts to their own
//! Menu; `build()` refuses a variant naming what nobody registered.

mod menu_variants_support;
mod roster_support;
mod support;

use harpseal::{BuildError, Message};
use harpseal_testing::{Harness, urlencode};
use menu_variants_support::*;
use roster_support::{confirm, import_id, run_jobs, signed_in, upload};
use support::Chat;

/// `to` says hello again: the Kernel sets its Menu and lists it.
async fn welcome_back(h: &Harness, app: &harpseal_testing::TestApp, id: &str) -> Vec<Message> {
    Chat::new(h, app, &format!("u-{id}")).follow().await
}

fn choices(msgs: &[Message]) -> Vec<String> {
    msgs.iter()
        .find_map(|m| match m {
            Message::Choices(c) => Some(support::actions_of(&Message::Choices(c.clone()))),
            _ => None,
        })
        .expect("the Menu is listed")
}

#[tokio::test]
async fn a_graduate_gets_the_variants_menu_and_choices() {
    let (h, app) = boot().await;
    let cur = linked(&h, "c1", 1, "current").await;
    let grad = linked(&h, "g1", 2, "graduated").await;
    let seen_cur = welcome_back(&h, &app, "c1").await;
    let seen_grad = welcome_back(&h, &app, "g1").await;
    assert_eq!(actions(&h, &cur), WHOLE);
    assert_eq!(actions(&h, &grad), GRAD_ONLY);
    assert_eq!(choices(&seen_cur), WHOLE);
    assert_eq!(choices(&seen_grad), GRAD_ONLY);
    let (m_cur, m_grad) = (menu(&h, &cur), menu(&h, &grad));
    assert_eq!(m_cur.variant(), None);
    assert_eq!(m_cur.key().unwrap().as_str(), "th");
    assert_eq!(m_grad.variant(), Some("grad"));
    assert_eq!(m_grad.key().unwrap().as_str(), "thgrad");
}

#[tokio::test]
async fn a_staff_edit_of_the_status_relinks_the_menu() {
    let (h, app) = boot().await;
    let who = linked(&h, "c1", 1, "current").await;
    welcome_back(&h, &app, "c1").await;
    run_jobs(&h, &app).await;
    let b = signed_in(&h, &app, "Officer").await;
    let edit = b.get("/admin/students/c1/edit").await.body;
    let key = "name=\"version\" value=\"";
    let at = edit.find(key).expect("a version field") + key.len();
    let version = &edit[at..at + 64];
    let form = |status: &str| {
        format!(
            "display_name=Nok+c1&email={}&locale=th&attr.status={status}&version={version}",
            urlencode("nokc1@student.sealbay.example")
        )
    };
    let r = b.post("/admin/students/c1/edit", &form("graduated")).await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(actions(&h, &who), WHOLE, "not before the Job runs");
    run_jobs(&h, &app).await;
    assert_eq!(actions(&h, &who), GRAD_ONLY);
}

#[tokio::test]
async fn a_staff_edit_of_another_field_queues_nothing() {
    let (h, app) = boot().await;
    linked(&h, "c1", 1, "current").await;
    let b = signed_in(&h, &app, "Officer").await;
    let edit = b.get("/admin/students/c1/edit").await.body;
    let key = "name=\"version\" value=\"";
    let at = edit.find(key).expect("a version field") + key.len();
    let form = format!(
        "display_name=Nok+Thale&email={}&locale=th&attr.status=current&version={}",
        urlencode("nokc1@student.sealbay.example"),
        &edit[at..at + 64]
    );
    let r = b.post("/admin/students/c1/edit", &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    let queued = app
        .count("SELECT count(*) AS n FROM job WHERE kind = 'kernel.menu.variants'")
        .await;
    assert_eq!(queued, 0);
}

#[tokio::test]
async fn a_roster_import_relinks_the_students_it_changed() {
    let (h, app) = boot().await;
    let cur = linked(&h, "c1", 1, "current").await;
    let grad = linked(&h, "g1", 2, "graduated").await;
    welcome_back(&h, &app, "c1").await;
    welcome_back(&h, &app, "g1").await;
    let b = signed_in(&h, &app, "Officer").await;
    let file = "รหัสนักศึกษา,ชื่อ-สกุล,อีเมล,สถานะ\r\n\
        68010001,Nok c1,nokc1@student.sealbay.example,graduated\r\n\
        68010002,Nok g1,nokg1@student.sealbay.example,current\r\n";
    let file = file.as_bytes();
    let r = upload(&b, "a.csv", file, "upsert").await;
    assert_eq!(r.status, 303, "{}", r.body);
    let r = confirm(&b, &import_id(&r)).await;
    assert_eq!(r.status, 303, "{}", r.body);
    run_jobs(&h, &app).await;
    assert_eq!(actions(&h, &cur), GRAD_ONLY);
    assert_eq!(actions(&h, &grad), WHOLE);
    assert_eq!(menu(&h, &grad).variant(), None);
}

#[tokio::test]
async fn a_label_save_relinks_each_account_to_its_own_menu() {
    let (h, app) = boot().await;
    let cur = linked(&h, "c1", 1, "current").await;
    let grad = linked(&h, "g1", 2, "graduated").await;
    welcome_back(&h, &app, "c1").await;
    welcome_back(&h, &app, "g1").await;
    let b = signed_in(&h, &app, "Officer").await;
    let form = format!("locale=th&value={}&version=-&op=save", urlencode("ข่าวล่าสุด"));
    let r = b.post("/admin/texts/tiles.news", &form).await;
    assert_eq!(r.status, 303, "{}", r.body);
    run_jobs(&h, &app).await;
    let (m_cur, m_grad) = (menu(&h, &cur), menu(&h, &grad));
    assert_eq!(m_cur.entries[0].label, "ข่าวล่าสุด");
    assert_eq!(m_cur.revision(), 1);
    assert_eq!(
        m_cur.variant(),
        None,
        "the whole Menu's re-link left it whole"
    );
    assert_eq!(m_grad.entries[0].label, "ข่าวล่าสุด");
    assert_eq!(
        m_grad.variant(),
        Some("grad"),
        "not moved to the whole Menu"
    );
    assert_eq!(m_grad.revision(), 0, "a variant Menu is never revised");
    assert_eq!(actions(&h, &grad), GRAD_ONLY);
}

#[tokio::test]
async fn the_mini_app_home_lists_only_the_variants_links() {
    let (h, app) = boot().await;
    linked(&h, "c1", 1, "current").await;
    linked(&h, "g1", 2, "graduated").await;
    let home = |id: &'static str| {
        let (h, app) = (&h, &app);
        async move {
            let b = app.browser();
            let r = b
                .open_mini_app(&h.fake_channel(), &format!("u-{id}"), "/app")
                .await;
            assert_eq!(r.status, 303, "{}", r.body);
            let page = b.get("/app").await.body;
            let fees = b.get(FEES).await.body;
            (page, fees)
        }
    };
    let (page, fees) = home("c1").await;
    assert!(page.contains(NEWS) && page.contains(FEES), "{page}");
    assert_eq!(fees, "variant=None");
    let (page, fees) = home("g1").await;
    assert!(page.contains(NEWS), "{page}");
    assert!(!page.contains(FEES), "{page}");
    assert_eq!(fees, "variant=Some(\"grad\")", "the page decides");
}

#[test]
fn build_refuses_a_variant_naming_nothing_registered() {
    let h = Harness::new();
    let cases = [
        (
            GRAD.replace("entries = [\"tiles.news\"]", "entries = [\"tiles.nope\"]"),
            "no Menu entry has Action `tiles.nope`",
        ),
        (
            GRAD.replace(NEWS, "/app/tiles/nope"),
            "no Mini App home link to `/app/tiles/nope`",
        ),
    ];
    for (toml, want) in cases {
        let e = h
            .builder()
            .config(config(&toml))
            .module(Tiles)
            .build()
            .unwrap_err();
        assert!(matches!(e, BuildError::InvalidConfig(_)), "{e:?}");
        assert!(e.to_string().contains(want), "{e}");
    }
}
