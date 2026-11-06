//! `build()` refusals for Action prefixes (`ModuleRegistrar::action_prefix`)
//! and for the Announcements Module on a Channel without a Mini App, and a
//! prefix Action reaching its handler with the whole id.

mod support;

use harpseal::modules::announcements::Announcements;
use harpseal::{
    ActionContext, ActionId, BuildError, ChannelKind, Deployment, EmailOtp, Message, Module,
    ModuleRegistrar, Statement,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{FAKE_SECRET, Harness};

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

async fn echo(cx: ActionContext) -> harpseal::Result<Vec<Message>> {
    Ok(vec![Message::text(cx.action.as_str())])
}

fn refused(m: TestModule, prefix: &str) {
    let h = Harness::new();
    let err = h
        .builder()
        .module(m)
        .build()
        .expect_err("build must refuse");
    assert_eq!(
        err,
        BuildError::DuplicateAction(ActionId::new(prefix).unwrap())
    );
}

#[test]
fn a_prefix_must_end_with_a_dot() {
    refused(
        module("p", |r| {
            r.action_prefix("p.ack", echo);
        }),
        "p.ack",
    );
}

#[test]
fn overlapping_prefixes_are_refused() {
    refused(
        module("p", |r| {
            r.action_prefix("p.ack.", echo).action_prefix("p.", echo);
        }),
        "p.",
    );
}

#[test]
fn a_prefix_covering_an_exact_id_of_any_module_is_refused() {
    refused(
        module("p", |r| {
            r.action(ActionId::new("p.ack.one").unwrap(), echo)
                .action_prefix("p.ack.", echo);
        }),
        "p.ack.",
    );
    let h = Harness::new();
    let err = h
        .builder()
        .module(module("q", |r| {
            r.action(ActionId::new("p.ack.x").unwrap(), echo);
        }))
        .module(module("p", |r| {
            r.action_prefix("p.ack.", echo);
        }))
        .build()
        .expect_err("another module's exact id");
    assert_eq!(
        err,
        BuildError::DuplicateAction(ActionId::new("p.ack.").unwrap())
    );
}

#[test]
fn announcements_on_a_channel_without_a_mini_app_is_refused() {
    let h = Harness::new();
    let b = Deployment::builder()
        .config(Harness::config())
        .runtime(h.services())
        .channel(FakeChannel::new(FAKE_SECRET).unwrap().without_mini_app())
        .verification(EmailOtp::default())
        .module(Announcements);
    assert_eq!(
        b.build().expect_err("no Mini App"),
        BuildError::MiniAppUnavailable {
            module: "announcements",
            channel: ChannelKind("fake"),
        }
    );
}

#[tokio::test]
async fn a_prefixed_action_reaches_its_handler_with_the_whole_id() {
    let h = Harness::new().with_fake_profile(FakeProfile::Full);
    let d = h
        .builder()
        .module(support::Hello)
        .module(module("p", |r| {
            r.action_prefix("p.ack.", echo);
        }))
        .build()
        .unwrap();
    let app = h.boot(d).await;
    for sql in support::ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    let mut chat = support::Chat::new(&h, &app, "u1");
    chat.link("68010001").await;
    let seen = chat.press("p.ack.42").await;
    assert_eq!(support::text_of(&seen[0]), "p.ack.42");
    // The bare prefix has no suffix: not this handler's.
    let bare = chat.press("p.ack.").await;
    assert_eq!(
        support::text_of(&bare[0]),
        "ปุ่มนี้ใช้ไม่ได้แล้ว กรุณาเลือกจากเมนูอีกครั้ง"
    );
}
