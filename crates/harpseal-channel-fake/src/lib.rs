//! Fake Channel: in-memory adapter for tests and demos. Implements the
//! Channel trait of BOTH prototype designs over one shared state, and
//! declares every optional Capability so the Core's paths can be exercised.
//! Metering mimics LINE: Reply is free, Push is metered.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use harpseal::delivery::{
    ChannelAccountId, ChannelKind, Delivery, Inbound, IncomingFile, Metering, PlatformMessageId,
    SendReceipt, WebhookRequest,
};
use harpseal::message::{Locale, Menu, Message};
use harpseal::{Error, Result};

pub const KIND: ChannelKind = ChannelKind("fake");
const SIGNATURE_HEADER: &str = "x-fake-signature";

#[derive(Debug, Default)]
pub struct FakeState {
    pub inbox: Vec<Inbound>,
    pub files: HashMap<String, Vec<u8>>,
    pub sent: Vec<(ChannelAccountId, Delivery, Message, PlatformMessageId)>,
    pub menus: HashMap<ChannelAccountId, Option<Menu>>,
    pub edited: Vec<(PlatformMessageId, Message)>,
    pub deleted: Vec<PlatformMessageId>,
}

/// Cloning shares the state, so a test keeps a handle after handing one to
/// the Deployment.
#[derive(Debug, Clone, Default)]
pub struct FakeChannel {
    secret: String,
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeChannel {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            state: Arc::default(),
        }
    }

    /// Queue an event for the next webhook call.
    pub fn push_inbound(&self, ev: Inbound) {
        self.lock().inbox.push(ev);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn do_parse(&self, req: &WebhookRequest) -> Result<Vec<Inbound>> {
        let sig = req
            .headers
            .get(SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok());
        if sig != Some(self.secret.as_str()) {
            return Err(Error::BadSignature);
        }
        Ok(std::mem::take(&mut self.lock().inbox))
    }

    fn do_download(&self, file: &IncomingFile) -> Result<Vec<u8>> {
        self.lock()
            .files
            .get(&file.platform_file_id)
            .cloned()
            .ok_or_else(|| Error::Platform("no such file".into()))
    }

    fn do_send(&self, to: &ChannelAccountId, d: &Delivery, m: &Message) -> SendReceipt {
        let mut s = self.lock();
        let id = PlatformMessageId(format!("fake-{}", s.sent.len()));
        s.sent.push((to.clone(), d.clone(), m.clone(), id.clone()));
        let metering = match d {
            Delivery::Reply(_) => Metering::Free,
            Delivery::Push => Metering::Metered,
        };
        SendReceipt {
            message_id: Some(id),
            metering,
        }
    }

    fn do_set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) {
        self.lock().menus.insert(to.clone(), menu.cloned());
    }

    fn launch(path: &str) -> String {
        format!("https://fake.invalid/app{path}")
    }

    fn session(token: &str) -> ChannelAccountId {
        ChannelAccountId {
            channel: KIND,
            external_id: token.to_string(),
        }
    }
}

mod a {
    use super::*;
    use harpseal as a;

    #[async_trait::async_trait]
    impl a::Channel for FakeChannel {
        fn kind(&self) -> ChannelKind {
            KIND
        }
        async fn parse_webhook(&self, req: WebhookRequest) -> Result<Vec<Inbound>> {
            self.do_parse(&req)
        }
        async fn download(&self, file: &IncomingFile) -> Result<Vec<u8>> {
            self.do_download(file)
        }
        async fn send(
            &self,
            to: &ChannelAccountId,
            d: &Delivery,
            m: &Message,
            _: &Locale,
        ) -> Result<SendReceipt> {
            Ok(self.do_send(to, d, m))
        }
        async fn set_menu(&self, to: &ChannelAccountId, menu: Option<&Menu>) -> Result<()> {
            self.do_set_menu(to, menu);
            Ok(())
        }
        fn editor(&self) -> Option<&dyn a::EditMessage> {
            Some(self)
        }
        fn deleter(&self) -> Option<&dyn a::DeleteMessage> {
            Some(self)
        }
        fn mini_app(&self) -> Option<&dyn a::MiniApp> {
            Some(self)
        }
    }

    #[async_trait::async_trait]
    impl a::EditMessage for FakeChannel {
        async fn edit(
            &self,
            _: &ChannelAccountId,
            id: &PlatformMessageId,
            m: &Message,
            _: &Locale,
        ) -> Result<()> {
            self.lock().edited.push((id.clone(), m.clone()));
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl a::DeleteMessage for FakeChannel {
        async fn delete(&self, _: &ChannelAccountId, id: &PlatformMessageId) -> Result<()> {
            self.lock().deleted.push(id.clone());
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl a::MiniApp for FakeChannel {
        fn launch_url(&self, path: &str) -> String {
            FakeChannel::launch(path)
        }
        async fn verify_session(&self, token: &str) -> Result<ChannelAccountId> {
            Ok(FakeChannel::session(token))
        }
    }
}
