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
    let b = h.builder().channel(FakeChannel::new(FAKE_SECRET).unwrap());
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
        r.back_office("reports.view", "/reports", get(|| async { "secret" }));
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
        .channel(FakeChannel::new(FAKE_SECRET).unwrap().without_mini_app())
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
            source: "module `news`".into(),
            key: "news.menu.latest".into(),
        }
    );
}

#[test]
fn toml_text_override_with_unknown_key() {
    let h = Harness::new();
    let toml = "name = \"Seal Bay University\"\ndefault_locale = \"th\"\n\
                [texts.th]\n\"kernel.notice.bdoy\" = \"typo\"\n";
    let b = h
        .builder()
        .config(DeploymentConfig::from_toml(toml).unwrap());
    assert_eq!(
        build_err(b),
        BuildError::UnknownText {
            source: "deployment.toml [texts.th]".into(),
            key: "kernel.notice.bdoy".into(),
        }
    );
}

#[test]
fn toml_text_override_of_a_known_key_builds() {
    let h = Harness::new();
    let toml = "name = \"Seal Bay University\"\ndefault_locale = \"th\"\n\
                [texts.en]\n\"kernel.notice.title\" = \"Privacy\"\n";
    let b = h
        .builder()
        .config(DeploymentConfig::from_toml(toml).unwrap());
    assert!(b.build().is_ok());
}

#[test]
fn duplicate_module_name() {
    let h = Harness::new();
    let b = h
        .builder()
        .module(module("news", |_| {}))
        .module(module("news", |_| {}));
    assert_eq!(build_err(b), BuildError::DuplicateModule("news".into()));
}

#[test]
fn a_module_may_not_call_itself_kernel() {
    let h = Harness::new();
    let b = h.builder().module(module("kernel", |_| {}));
    assert_eq!(build_err(b), BuildError::DuplicateModule("kernel".into()));
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

#[test]
fn overlapping_routes_are_refused_at_build_time() {
    let h = Harness::new();
    let b = h
        .builder()
        .module(module("news", |r| {
            r.public("/news/{id}", get(|| async { "a" }));
        }))
        .module(module("events", |r| {
            r.public("/news/{slug}", get(|| async { "b" }));
        }));
    assert_eq!(
        build_err(b),
        BuildError::DuplicateRoute {
            module: "events",
            path: "/news/{slug}".into(),
            first: "news",
        }
    );
    let kernel = h.builder().module(module("hooks", |r| {
        r.public("/webhook/{channel}", get(|| async { "x" }));
    }));
    assert!(matches!(
        build_err(kernel),
        BuildError::DuplicateRoute {
            first: "kernel",
            ..
        }
    ));
}

#[test]
fn public_routes_under_admin_or_in_old_syntax_are_refused() {
    let h = Harness::new();
    struct Sneaky(&'static str);
    impl Module for Sneaky {
        fn name(&self) -> &'static str {
            "sneaky"
        }
        fn register(&self, r: &mut ModuleRegistrar) {
            r.public(self.0, get(|| async { "secret" }));
        }
    }
    for path in ["/admin/secrets", "/admin", "/files/:id", "relative"] {
        let b = h.builder().module(Sneaky(path));
        assert_eq!(
            build_err(b),
            BuildError::InvalidRoute {
                module: "sneaky",
                path: path.into(),
            },
            "{path}"
        );
    }
}
