//! The back-office polish pass (#131): untitled Modules are told apart,
//! the Fake Channel shows only in dev mode, and a submit shows that it is
//! busy and cannot be pressed twice.

mod admin_support;
mod support;

use admin_support::{CONFIG, signed_in};
use harpseal::{CatalogEntry, DeploymentConfig, Module, ModuleRegistrar, Permission, Statement};
use harpseal_testing::{Harness, TestApp};

/// A Module with a Permission and a text, but no title, menu text or
/// back-office page: the Core can only call it "ส่วนเสริม".
struct Untitled(&'static str);

impl Module for Untitled {
    fn name(&self) -> &'static str {
        self.0
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        match self.0 {
            "alpha" => {
                r.permission(Permission::new("alpha.thing.view", "View alpha things"));
                r.messages(&[CatalogEntry::new("alpha.greeting", "th", "สวัสดี")]);
            }
            _ => {
                r.permission(Permission::new("beta.thing.view", "View beta things"));
                r.messages(&[CatalogEntry::new("beta.greeting", "th", "สวัสดีครับ")]);
            }
        }
    }
}

async fn boot_with(h: &Harness, dev: bool, untitled: bool) -> TestApp {
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let mut b = h.builder().dev(dev).config(config);
    if untitled {
        b = b.module(Untitled("alpha")).module(Untitled("beta"));
    }
    let app = h.boot(b.build().unwrap()).await;
    for sql in support::ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    app
}

#[tokio::test]
async fn untitled_modules_read_apart_on_roles_and_in_the_texts_filter() {
    let h = Harness::new();
    let app = boot_with(&h, true, true).await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    let roles = admin.get("/admin/roles").await.body;
    for m in ["alpha", "beta"] {
        assert!(
            roles.contains(&format!("<legend>ส่วนเสริม ({m})</legend>")),
            "{roles}"
        );
    }
    assert!(!roles.contains("<legend>ส่วนเสริม</legend>"), "{roles}");
    let texts = admin.get("/admin/texts").await.body;
    assert!(
        texts.contains("<option value=\"alpha\">ส่วนเสริม (alpha)</option>"),
        "{texts}"
    );
    assert!(
        texts.contains("<option value=\"beta\">ส่วนเสริม (beta)</option>"),
        "{texts}"
    );
}

#[tokio::test]
async fn the_fake_channel_shows_only_in_dev_mode() {
    for dev in [true, false] {
        let h = Harness::new();
        let app = boot_with(&h, dev, false).await;
        let (admin, _) = signed_in(&h, &app, "Administrator").await;
        let list = admin.get("/admin/students").await.body;
        assert_eq!(list.contains("fake: "), dev, "dev {dev}: {list}");
        let id = h
            .db()
            .query(Statement::new(
                "SELECT id FROM student ORDER BY student_code LIMIT 1",
            ))
            .await
            .unwrap();
        let id = id
            .get(0, "id")
            .and_then(|v| v.as_text())
            .unwrap()
            .to_string();
        let detail = admin.get(&format!("/admin/students/{id}")).await.body;
        assert_eq!(detail.contains("fake: "), dev, "dev {dev}: {detail}");
        assert_eq!(detail.contains("<option value=\"fake\">"), dev, "{detail}");
        let usage = admin.get("/admin/usage").await.body;
        assert_eq!(usage.contains("(fake)"), dev, "dev {dev}: {usage}");
    }
}

#[tokio::test]
async fn main_forms_carry_the_busy_state() {
    let h = Harness::new();
    let app = boot_with(&h, true, false).await;
    let (admin, _) = signed_in(&h, &app, "Administrator").await;
    // The script is on every back-office page, once, after the content.
    for path in [
        "/admin",
        "/admin/roles",
        "/admin/students",
        "/admin/staff/new",
    ] {
        let page = admin.get(path).await.body;
        assert_eq!(
            page.matches("setTimeout(function(){hold(f,by)").count(),
            1,
            "{path}"
        );
        assert!(
            page.trim_end().ends_with("</script></body></html>"),
            "{path}"
        );
    }
    // The main forms post with a real submit button the script can hold,
    // and none opts out.
    for (path, form) in [
        ("/admin/roles", "action=\"/admin/roles\""),
        ("/admin/staff/new", "action=\"/admin/staff/new\""),
    ] {
        let page = admin.get(path).await.body;
        let at = page.find(form).unwrap_or_else(|| panic!("{path}: {page}"));
        let rest = &page[at..];
        let end = rest.find("</form>").unwrap();
        assert!(
            rest[..end].contains("type=\"submit\""),
            "{path}: {}",
            &rest[..end]
        );
        assert!(!page.contains(" data-no-busy"), "{path}");
    }
    // The states come from the stylesheet's tokens.
    let css = app.get(harpseal::ui::CSS_PATH).await.body;
    for rule in [
        "[data-busy]::after",
        "form[aria-busy=\"true\"]",
        "form.htmx-request button[type=\"submit\"]::after",
        ":is(button, .btn):disabled",
        "--brand-press",
        "prefers-reduced-motion",
    ] {
        assert!(css.contains(rule), "{rule}");
    }
    // The Mini App gets no busy script: it is not a back-office page.
    let notice = app.get("/notice").await.body;
    assert!(!notice.contains("aria-busy"), "{notice}");
}
