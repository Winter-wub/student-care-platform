//! One test per `BuildError` variant, written through `Deployment::builder()`.

use axum::routing::get;
use harpseal::{
    ActionContext, ActionId, BuildError, CatalogEntry, ChannelKind, Deployment, DeploymentConfig,
    EmailOtp, Job, JobContext, JobSpec, MenuEntrySpec, Module, ModuleRegistrar, Part, Permission,
};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{FAKE_SECRET, Harness};

/// A Module made of closures over the registrar, so each test states only
/// the registration that matters.
struct TestModule {
    name: &'static str,
    register: fn(&mut ModuleRegistrar),
}

impl Module for TestModule {
    fn name(&self) -> &'static str {
        self.name
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        (self.register)(r)
    }
}

fn module(name: &'static str, register: fn(&mut ModuleRegistrar)) -> TestModule {
    TestModule { name, register }
}

fn id(s: &str) -> ActionId {
    ActionId::new(s).unwrap()
}

async fn nothing(_: ActionContext) -> harpseal::Result<Vec<harpseal::Message>> {
    Ok(Vec::new())
}

async fn no_job(_: JobContext, _: Job) -> harpseal::Result<()> {
    Ok(())
}

fn build_err(b: harpseal::DeploymentBuilder) -> BuildError {
    b.build().expect_err("build must refuse")
}

#[test]
fn the_harness_deployment_builds() {
    assert!(Harness::new().builder().build().is_ok());
}

#[test]
fn missing_config() {
    let h = Harness::new();
    let b = Deployment::builder()
        .runtime(h.services())
        .channel(h.fake_channel())
        .verification(EmailOtp::default());
    assert_eq!(build_err(b), BuildError::Missing(Part::Config));
}

#[test]
fn missing_runtime() {
    let h = Harness::new();
    let b = Deployment::builder()
        .config(Harness::config())
        .channel(h.fake_channel())
        .verification(EmailOtp::default());
    assert_eq!(build_err(b), BuildError::Missing(Part::Runtime));
}

#[test]
fn missing_verification() {
    let h = Harness::new();
    let b = Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .channel(h.fake_channel());
    assert_eq!(build_err(b), BuildError::Missing(Part::Verification));
}

#[test]
fn missing_channel() {
    let h = Harness::new();
    let b = Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .verification(EmailOtp::default());
    assert_eq!(build_err(b), BuildError::Missing(Part::Channel));
}

#[test]
fn duplicate_channel_kind() {
    let h = Harness::new();
    let b = h.builder().channel(FakeChannel::new(FAKE_SECRET));
    assert_eq!(
        build_err(b),
        BuildError::DuplicateChannel(ChannelKind("fake"))
    );
}

#[test]
fn duplicate_action_id_across_modules() {
    let h = Harness::new();
    let b = h
        .builder()
        .module(module("a", |r| {
            r.action(id("shared.press"), nothing);
        }))
        .module(module("b", |r| {
            r.action(id("shared.press"), nothing);
        }));
    assert_eq!(
        build_err(b),
        BuildError::DuplicateAction(id("shared.press"))
    );
}

#[test]
fn duplicate_permission_with_the_kernel() {
    let h = Harness::new();
    let b = h.builder().module(module("dup", |r| {
        r.permission(Permission::new("kernel.student.view", "again"));
    }));
    assert_eq!(
        build_err(b),
        BuildError::DuplicatePermission("kernel.student.view".into())
    );
}

#[test]
fn duplicate_job_kind() {
    let h = Harness::new();
    let spec = JobSpec {
        kind: "jobs.tick",
        schedule: None,
    };
    let b = h.builder().module(module("jobs", |r| {
        let spec = JobSpec {
            kind: "jobs.tick",
            schedule: None,
        };
        r.job(spec, no_job).job(spec, no_job);
    }));
    assert_eq!(build_err(b), BuildError::DuplicateJob(spec.kind.into()));
}

#[test]
fn role_with_unknown_permission() {
    let h = Harness::new();
    let toml = "name = \"Seal Bay University\"\ndefault_locale = \"th\"\n\
                [roles]\nOfficer = [\"kernel.student.view\", \"fees.adjust\"]\n";
    let config = DeploymentConfig::from_toml(toml).unwrap();
    let b = h.builder().config(config);
    assert_eq!(
        build_err(b),
        BuildError::UnknownPermission {
            role: "Officer".into(),
            permission: "fees.adjust".into(),
        }
    );
}

#[test]
fn back_office_route_with_unknown_permission() {
    let h = Harness::new();
    let b = h.builder().module(module("reports", |r| {
        let pages = axum::Router::new().route("/reports", get(|| async { "secret" }));
        r.back_office("reports.view", pages);
    }));
    assert_eq!(
        build_err(b),
        BuildError::UnguardedRoute {
            module: "reports",
            permission: "reports.view",
        }
    );
}

#[test]
fn mini_app_button_on_a_channel_without_mini_app() {
    let h = Harness::new();
    let b = Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .channel(FakeChannel::new(FAKE_SECRET).without_mini_app())
        .verification(EmailOtp::default())
        .module(module("news", |r| {
            r.mini_app_button("/app");
        }));
    assert_eq!(
        build_err(b),
        BuildError::MiniAppUnavailable {
            module: "news",
            channel: ChannelKind("fake"),
        }
    );
}

#[test]
fn mini_app_button_is_fine_when_every_channel_has_one() {
    let h = Harness::new();
    let b = h.builder().module(module("news", |r| {
        r.mini_app_button("/app");
    }));
    assert!(b.build().is_ok());
}

#[test]
fn menu_entry_with_unknown_text_key() {
    let h = Harness::new();
    let b = h.builder().module(module("news", |r| {
        let spec = MenuEntrySpec {
            label_key: "news.menu.latest",
            action: id("news.latest"),
            order: 1,
        };
        // An English text alone does not cover the Thai default Locale.
        r.messages(&[CatalogEntry::new("news.menu.latest", "en", "Latest news")])
            .menu_entry(spec, nothing);
    }));
    assert_eq!(
        build_err(b),
        BuildError::UnknownText {
            module: "news",
            key: "news.menu.latest",
        }
    );
}

#[test]
fn build_errors_explain_themselves() {
    let e = BuildError::MiniAppUnavailable {
        module: "news",
        channel: ChannelKind("line"),
    };
    assert_eq!(
        e.to_string(),
        "module `news` sends Mini App buttons but channel `line` has no Mini App URL"
    );
}
