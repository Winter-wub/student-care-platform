//! Shared setup for the chat-flow tests: a roster, a small Module with a
//! Menu entry, and a `Chat` that plays one Student on the Fake Channel.

#![allow(dead_code)]

use harpseal::{
    ActionContext, ActionId, CatalogEntry, ChannelAccountId, Inbound, IncomingFile, Menu,
    MenuEntrySpec, Message, Module, ModuleRegistrar, ReplyToken, Statement, Timestamp,
};
use harpseal_channel_fake::{FakeChannel, FakeProfile};
use harpseal_testing::{Harness, TestApp};

/// Roster: an active Thai Student, a left one, one without email, an
/// active English one.
pub const ROSTER: &[&str] = &[
    "INSERT INTO student (id, student_code, display_name, email, locale, status, created_at, updated_at) \
     VALUES ('s1', '68010001', 'Nok Thale', 's68010001@student.sealbay.example', 'th', 'active', 1, 1)",
    "INSERT INTO student (id, student_code, display_name, email, locale, status, created_at, updated_at) \
     VALUES ('s2', '68010002', 'Pla Kapong', 's68010002@student.sealbay.example', 'th', 'left', 1, 1)",
    "INSERT INTO student (id, student_code, display_name, email, locale, status, created_at, updated_at) \
     VALUES ('s3', '68010003', 'Hoi Nangrom', NULL, 'th', 'active', 1, 1)",
    "INSERT INTO student (id, student_code, display_name, email, locale, status, created_at, updated_at) \
     VALUES ('s4', '68010004', 'Karn Bai-ruea', 's68010004@student.sealbay.example', 'en', 'active', 1, 1)",
];

/// A Module with one Menu entry, `hello.say`.
pub struct Hello;

impl Module for Hello {
    fn name(&self) -> &'static str {
        "hello"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(&[
            CatalogEntry::new("hello.menu", "th", "ทักทาย"),
            CatalogEntry::new("hello.menu", "en", "Say hello"),
            CatalogEntry::new("hello.hi", "th", "สวัสดีจากโมดูล"),
            CatalogEntry::new("hello.hi", "en", "Hello from the Module"),
        ])
        .menu_entry(
            MenuEntrySpec {
                label_key: "hello.menu",
                action: ActionId::new("hello.say").unwrap(),
                order: 1,
            },
            |cx: ActionContext| async move { Ok(vec![Message::text(cx.t("hello.hi"))]) },
        );
    }
}

/// A Deployment with [`Hello`], the roster loaded, the Fake Channel in
/// `profile`.
pub async fn boot(profile: FakeProfile) -> (Harness, TestApp) {
    let h = Harness::new().with_fake_profile(profile);
    let app = h.boot(h.builder().module(Hello).build().unwrap()).await;
    for sql in ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    (h, app)
}

/// One Student on the Fake Channel.
pub struct Chat<'a> {
    pub h: &'a Harness,
    pub app: &'a TestApp,
    pub who: ChannelAccountId,
    n: u32,
}

impl<'a> Chat<'a> {
    pub fn new(h: &'a Harness, app: &'a TestApp, external_id: &str) -> Self {
        Self {
            h,
            app,
            who: FakeChannel::account(external_id),
            n: 0,
        }
    }

    fn token(&mut self) -> Option<ReplyToken> {
        self.n += 1;
        Some(ReplyToken {
            token: format!("{}-r{}", self.who.external_id, self.n),
            expires_at: Timestamp(self.h.clock().now_ms() + 60_000),
        })
    }

    /// Sends `ev` and returns what this Student saw because of it.
    pub async fn send(&mut self, ev: Inbound) -> Vec<Message> {
        self.h.fake_channel().take_sent();
        let r = self.app.fake_webhook([ev]).await;
        assert_eq!(r.status, 200, "{}", r.body);
        self.seen_since()
    }

    /// Messages this Student saw since the last call (others' kept apart).
    pub fn seen_since(&self) -> Vec<Message> {
        self.h
            .fake_channel()
            .sent()
            .into_iter()
            .filter(|s| s.to == self.who)
            .map(|s| s.message)
            .collect()
    }

    pub async fn follow(&mut self) -> Vec<Message> {
        let reply = self.token();
        self.send(Inbound::Follow {
            from: self.who.clone(),
            reply,
        })
        .await
    }
    pub async fn text(&mut self, text: &str) -> Vec<Message> {
        let reply = self.token();
        let ev = Inbound::Text {
            from: self.who.clone(),
            text: text.into(),
            reply,
        };
        self.send(ev).await
    }
    pub async fn press(&mut self, action: &str) -> Vec<Message> {
        let reply = self.token();
        let action = ActionId::new(action).unwrap();
        self.send(Inbound::Action {
            from: self.who.clone(),
            action,
            reply,
        })
        .await
    }
    pub async fn file(&mut self, file: IncomingFile) -> Vec<Message> {
        let reply = self.token();
        self.send(Inbound::File {
            from: self.who.clone(),
            file,
            reply,
        })
        .await
    }

    /// The code in the last email (the subject ends with it).
    pub fn code(&self) -> String {
        let email = self.h.email().last().expect("an OTP email was sent");
        email.subject.rsplit(' ').next().unwrap().to_string()
    }

    /// Follow, accept, student id, the emailed code: linked.
    pub async fn link(&mut self, code: &str) -> Vec<Message> {
        self.follow().await;
        self.press("kernel.consent.accept").await;
        self.text(code).await;
        let otp = self.code();
        self.text(&otp).await
    }

    /// The Menu as the Channel has it: `None` = never set, `Some(None)` =
    /// locked, `Some(Some(menu))` = unlocked.
    pub fn menu(&self) -> Option<Option<Menu>> {
        self.h.fake_channel().menu(&self.who)
    }
}

/// The text of a Text, Card body, or Choices prompt.
pub fn text_of(m: &Message) -> String {
    match m {
        Message::Text(t) => t.clone(),
        Message::Card(c) => format!("{}\n{}", c.title, c.body),
        Message::Choices(c) => c.text.clone(),
        other => format!("{other:?}"),
    }
}

/// Action ids of every button in `m`.
pub fn actions_of(m: &Message) -> Vec<String> {
    m.buttons()
        .into_iter()
        .filter_map(|b| b.action_id().map(|a| a.as_str().to_string()))
        .collect()
}

/// A file the Fake Channel can serve.
pub fn upload(h: &Harness, id: &str, bytes: Vec<u8>, size: Option<u64>) -> IncomingFile {
    h.fake_channel().add_file(id, bytes);
    IncomingFile {
        platform_file_id: id.into(),
        file_name: Some("ใบลา.pdf".into()),
        content_type: Some("application/pdf".into()),
        size_bytes: size,
    }
}

/// [`boot`] with extra TOML appended to the test config and the Services
/// changed by `services` (e.g. a failing EmailSender).
pub async fn boot_with(
    profile: FakeProfile,
    toml: &str,
    services: impl FnOnce(harpseal::Services) -> harpseal::Services,
) -> (Harness, TestApp) {
    let h = Harness::new().with_fake_profile(profile);
    let config = harpseal::DeploymentConfig::from_toml(&format!(
        "{}\n{toml}",
        harpseal_testing::TEST_CONFIG
    ))
    .unwrap();
    let d = h
        .builder()
        .config(config)
        .runtime(services(h.services()))
        .module(Hello)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    for sql in ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    (h, app)
}

/// Runs one SQL statement on the harness database.
pub async fn sql(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// The Hello Module's answer.
pub const HELLO: &str = "สวัสดีจากโมดูล";
/// `kernel.flow.welcome_back` in Thai.
pub const WELCOME_BACK: &str = "ยินดีต้อนรับกลับมา บัญชีของคุณยังเชื่อมอยู่ ใช้เมนูได้ตามปกติ";
/// `kernel.link.revoked_by_staff` in Thai.
pub const REVOKED_NOTICE: &str = "เจ้าหน้าที่ยกเลิกการเชื่อมบัญชีนี้แล้ว หากต้องการเชื่อมใหม่ พิมพ์รหัสนักศึกษาได้ทุกเมื่อ";

/// A Staff member, as the audit Actor of back-office calls.
pub async fn officer(h: &Harness) -> harpseal::Actor {
    let id = h
        .create_staff("officer@sealbay.example", "correct horse battery", &[])
        .await;
    harpseal::Actor::Staff(id)
}

/// The id of the only live (active or unreachable) Link of `external_id`.
pub async fn live_link(h: &Harness, external_id: &str) -> String {
    let rows = h
        .db()
        .query(
            Statement::new(
                "SELECT l.id FROM link l JOIN channel_account ca ON ca.id = l.channel_account_id \
                 WHERE ca.external_id = ? AND l.status <> 'revoked'",
            )
            .bind(external_id),
        )
        .await
        .unwrap();
    rows.get(0, "id").unwrap().as_text().unwrap().to_string()
}

/// Column `col` of the first row, as text.
pub fn col(rows: &harpseal::Rows, col: &str) -> Option<String> {
    rows.get(0, col)
        .and_then(|v| v.as_text())
        .map(str::to_string)
}
