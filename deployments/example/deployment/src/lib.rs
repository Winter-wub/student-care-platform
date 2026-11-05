//! The fictional "Seal Bay University" Deployment, written once per
//! prototype design so the customer-facing code can be compared.
//! Same pieces both times: Thai default Locale, LINE + Telegram + Fake
//! channels, email-OTP Verification, and a tiny `hello` Module adding one
//! Menu entry and one back-office page.

use harpseal::config::DeploymentConfig;
use harpseal::manifest::{CatalogEntry, MenuEntrySpec, Permission};
use harpseal::message::ActionId;
use harpseal_channel_line::LineConfig;
use harpseal_channel_telegram::TelegramConfig;

pub const DEPLOYMENT_TOML: &str = include_str!("../deployment.toml");
const HELLO_SQL: &str = include_str!("../sql/0001_hello.sql");

pub fn config() -> DeploymentConfig {
    DeploymentConfig::from_toml(DEPLOYMENT_TOML).expect("deployment.toml is valid")
}

/// Secrets come from the environment / Worker secrets, never from TOML.
pub struct Secrets {
    pub line: LineConfig,
    pub telegram: TelegramConfig,
    pub fake_secret: String,
}

// Data shared by both versions of `hello` (plain consts either way).
const VIEW_HELLO: Permission = Permission { id: "hello.view", description: "See hello stats" };
const HELLO_TEXTS: &[CatalogEntry] = &[
    CatalogEntry { key: "hello.menu", locale: "th", text: "ทักทาย" },
    CatalogEntry { key: "hello.menu", locale: "en", text: "Say hello" },
    CatalogEntry { key: "hello.greeting", locale: "th", text: "สวัสดี" },
    CatalogEntry { key: "hello.greeting", locale: "en", text: "Hello" },
];

fn say_hello() -> MenuEntrySpec {
    let action = ActionId::new("hello.say").expect("Action id is ≤ 64 bytes");
    MenuEntrySpec { label_key: "hello.menu", action, order: 10 }
}

// ───────────────────────────── Design A ─────────────────────────────

pub mod with_design_a {
    use axum::extract::State;
    use axum::response::Html;
    use axum::routing::get;
    use harpseal::design_a::{ActionContext, AppState, Deployment, Module, ModuleRegistrar, Services};
    use harpseal::message::Message;
    use harpseal::port_types::Statement;
    use harpseal::verification::EmailOtp;
    use harpseal::BuildError;
    use harpseal_channel_fake::FakeChannel;
    use harpseal_channel_line::a::LineChannel;
    use harpseal_channel_telegram::a::TelegramChannel;

    use super::*;

    pub struct Hello;

    impl Module for Hello {
        fn name(&self) -> &'static str {
            "hello"
        }

        fn register(&self, r: &mut ModuleRegistrar) {
            r.permission(VIEW_HELLO)
                .messages(HELLO_TEXTS)
                .migration("0001_hello", HELLO_SQL)
                .menu_entry(say_hello(), |cx: ActionContext| async move {
                    let insert = Statement::new("INSERT INTO hello_greeting (student_id, at) VALUES (?, ?)")
                        .bind(cx.student.0.clone())
                        .bind(cx.services.clock.now().0);
                    cx.services.db.execute(insert).await?;
                    Ok(vec![Message::text(cx.t("hello.greeting"))])
                })
                .back_office("hello.view", axum::Router::new().route("/hello", get(hello_page)));
        }
    }

    async fn hello_page(State(s): State<AppState>) -> Html<String> {
        let n = s.services.db.query(Statement::new("SELECT count(*) FROM hello_greeting")).await;
        Html(format!("<h1>hello</h1><p>{} greetings</p>", n.map(|r| r.rows.len()).unwrap_or(0)))
    }

    /// The runtime crate supplies `services`; everything else is here.
    pub fn deployment(services: Services, secrets: Secrets) -> Result<Deployment, BuildError> {
        let http = services.http.clone();
        Deployment::builder()
            .config(config())
            .runtime(services)
            .channel(LineChannel { cfg: secrets.line, http: http.clone() })
            .channel(TelegramChannel { cfg: secrets.telegram, http })
            .channel(FakeChannel::new(secrets.fake_secret))
            .verification(EmailOtp::default())
            .module(Hello)
            .build()
    }
}

// ───────────────────────────── Design B ─────────────────────────────

pub mod with_design_b {
    use std::sync::Arc;

    use axum::extract::State;
    use axum::response::Html;
    use axum::routing::get;
    use harpseal::design_b::{ActionCx, BackOffice, BackOfficeState, Clock, Db, Deployment, HttpClient, Module, Runtime};
    use harpseal::manifest::Migration;
    use harpseal::message::Message;
    use harpseal::port_types::Statement;
    use harpseal::verification::EmailOtp;
    use harpseal::{BuildError, Result};
    use harpseal_channel_fake::FakeChannel;
    use harpseal_channel_line::b::LineChannel;
    use harpseal_channel_telegram::b::TelegramChannel;

    use super::*;

    pub struct Hello;

    impl<R: Runtime> Module<R> for Hello {
        const NAME: &'static str = "hello";
        const PERMISSIONS: &'static [Permission] = &[VIEW_HELLO];
        const CATALOG: &'static [CatalogEntry] = HELLO_TEXTS;
        const MIGRATIONS: &'static [Migration] =
            &[Migration { module: "hello", name: "0001_hello", sql: HELLO_SQL }];

        fn menu(&self) -> Vec<MenuEntrySpec> {
            vec![say_hello()]
        }

        fn back_office(&self) -> Option<(&'static str, BackOffice<R>)> {
            Some(("hello.view", axum::Router::new().route("/hello", get(hello_page::<R>))))
        }

        async fn on_action(&self, cx: &ActionCx<R>, action: &ActionId) -> Result<Option<Vec<Message>>> {
            match action.as_str() {
                "hello.say" => {
                    let insert = Statement::new("INSERT INTO hello_greeting (student_id, at) VALUES (?, ?)")
                        .bind(cx.student.0.clone())
                        .bind(cx.runtime.clock().now().0);
                    cx.runtime.db().execute(insert).await?;
                    Ok(Some(vec![Message::text(cx.t("hello.greeting"))]))
                }
                _ => Ok(None),
            }
        }
    }

    async fn hello_page<R: Runtime>(State(s): State<Arc<BackOfficeState<R>>>) -> Html<String> {
        let n = s.runtime.db().query(Statement::new("SELECT count(*) FROM hello_greeting")).await;
        Html(format!("<h1>hello</h1><p>{} greetings</p>", n.map(|r| r.rows.len()).unwrap_or(0)))
    }

    /// The full static type of this Deployment. The builder infers it; it is
    /// spelled out here only because a function must name its return type.
    pub type Example<R, H> = Deployment<
        R,
        (FakeChannel, (TelegramChannel<H>, (LineChannel<H>, ()))),
        (Hello, ()),
        EmailOtp,
    >;

    /// Generic over the runtime; the channel adapters need their own HTTP
    /// client value, hence `H: Clone`.
    pub fn deployment<R: Runtime, H: HttpClient + Clone>(
        runtime: R,
        http: H,
        secrets: Secrets,
    ) -> Result<Example<R, H>, BuildError> {
        Deployment::builder(runtime)
            .channel(LineChannel { cfg: secrets.line, http: http.clone() })
            .channel(TelegramChannel { cfg: secrets.telegram, http })
            .channel(FakeChannel::new(secrets.fake_secret))
            .verification(EmailOtp::default())
            .module(Hello)
            .build(config())
    }
}
