//! Setup for the Menu variant tests (#182): a `status` attribute, a
//! `grad` variant that keeps one of two Menu entries and one of two Mini
//! App home links, and linked Students inserted by SQL.

#![allow(dead_code)]

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use harpseal::{
    ActionContext, ActionId, AppState, AppStudent, CatalogEntry, ChannelAccountId,
    DeploymentConfig, MenuEntrySpec, Message, Module, ModuleRegistrar, Statement,
};
use harpseal_channel_fake::FakeChannel;
use harpseal_testing::{Harness, TestApp};

pub const CONFIG: &str = r#"
name = "Seal Bay University (menu variants)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Officer = ["kernel.student.view", "kernel.student.edit", "kernel.link.manage",
           "kernel.roster.view", "kernel.roster.import", "kernel.setting.edit",
           "kernel.texts.edit"]

[roster]
student_code = "รหัสนักศึกษา"
display_name = "ชื่อ-สกุล"
email = "อีเมล"

[roster.attributes.status]
column = "สถานะ"
type = "enum"
values = ["current", "graduated"]
label = { th = "สถานะ" }
"#;

/// The variant under test.
pub const GRAD: &str = r#"
[[menu.variants]]
key = "grad"
when = { status = ["graduated"] }
entries = ["tiles.news"]
app_links = ["/app/tiles/news"]
"#;

pub const NEWS: &str = "/app/tiles/news";
pub const FEES: &str = "/app/tiles/fees";

/// Two Menu entries (news first) and two Mini App pages linked from the
/// home; each page prints the Student's Menu variant.
pub struct Tiles;

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("tiles.news", "th", "ข่าวสาร"),
    CatalogEntry::new("tiles.fees", "th", "ค่าธรรมเนียม"),
    CatalogEntry::new("tiles.answer", "th", "รับทราบ"),
];

impl Module for Tiles {
    fn name(&self) -> &'static str {
        "tiles"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        let entry = |key: &'static str, order| MenuEntrySpec {
            label_key: key,
            action: ActionId::new(key).unwrap(),
            order,
        };
        let answer =
            |cx: ActionContext| async move { Ok(vec![Message::text(cx.t("tiles.answer"))]) };
        r.messages(TEXTS)
            .menu_entry(entry("tiles.news", 1), answer)
            .menu_entry(entry("tiles.fees", 2), answer)
            .public(NEWS, get(page))
            .public(FEES, get(page))
            .mini_app_link("tiles.news", NEWS, 10)
            .mini_app_link("tiles.fees", FEES, 20);
    }
}

async fn page(State(_): State<AppState>, me: AppStudent) -> Response {
    format!("variant={:?}", me.menu_variant).into_response()
}

/// The Deployment with `variants` appended to [`CONFIG`].
pub fn config(variants: &str) -> DeploymentConfig {
    DeploymentConfig::from_toml(&format!("{CONFIG}{variants}")).unwrap()
}

pub async fn boot() -> (Harness, TestApp) {
    let h = Harness::new();
    let d = h
        .builder()
        .config(config(GRAD))
        .module(Tiles)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    (h, app)
}

/// A Student `id` with `status`, linked on the Fake Channel as `u-<id>`
/// (code `6801<n>`), their Menu not yet set.
pub async fn linked(h: &Harness, id: &str, n: u32, status: &str) -> ChannelAccountId {
    let sql = [
        format!(
            "INSERT INTO student (id, student_code, display_name, email, locale, status, \
             attributes, created_at, updated_at) VALUES ('{id}', '6801{n:04}', 'Nok {id}', \
             'nok{id}@student.sealbay.example', 'th', 'active', '{{\"status\":\"{status}\"}}', 1, 1)"
        ),
        format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('ca-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
        ),
        format!(
            "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
             linked_at, updated_at) VALUES ('ln-{id}', '{id}', 'ca-{id}', 'fake', 'active', \
             'manual', 1, 1)"
        ),
    ];
    for s in sql {
        h.db().execute(Statement::new(s)).await.unwrap();
    }
    FakeChannel::account(format!("u-{id}"))
}

/// The Action ids of `to`'s unlocked Menu.
pub fn actions(h: &Harness, to: &ChannelAccountId) -> Vec<String> {
    menu(h, to)
        .entries
        .iter()
        .map(|e| e.action.as_str().to_string())
        .collect()
}

/// `to`'s unlocked Menu.
pub fn menu(h: &Harness, to: &ChannelAccountId) -> harpseal::Menu {
    h.fake_channel()
        .menu(to)
        .flatten()
        .expect("the Menu is unlocked")
}

pub const WHOLE: [&str; 3] = ["tiles.news", "tiles.fees", "kernel.link.unlink"];
pub const GRAD_ONLY: [&str; 2] = ["tiles.news", "kernel.link.unlink"];
