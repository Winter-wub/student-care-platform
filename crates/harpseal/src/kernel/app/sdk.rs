//! Which platform script the entry page needs for which Channel, as each
//! Channel's `MiniApp` Capability reports it ([`MiniApp::web_client`]):
//! the launch URL and the Channel kind play no part, so a LINE MINI App
//! (`https://miniapp.line.me/…`) and a Telegram Channel under another
//! kind get their scripts. A Channel that reports none gets no script
//! (the Fake Channel: tests post the token themselves).

use crate::channel::MiniApp;
use crate::{Deployment, WebClient};

/// The Mini App platforms of a Deployment.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sdks {
    /// (Channel kind, LIFF id) of the first LIFF Channel.
    pub(crate) liff: Option<(String, String)>,
    /// Channel kind of the first Telegram Web App Channel.
    pub(crate) telegram: Option<String>,
}

impl Sdks {
    pub(crate) fn of(d: &Deployment) -> Self {
        let mut out = Self::default();
        for kind in d.channel_kinds() {
            let Some(app) = d.channel(kind.as_str()).and_then(|c| c.mini_app()) else {
                continue;
            };
            let kind = kind.as_str().to_string();
            match app.web_client() {
                Some(WebClient::Liff { id }) if out.liff.is_none() && !id.is_empty() => {
                    out.liff = Some((kind, id));
                }
                Some(WebClient::Telegram) if out.telegram.is_none() => {
                    out.telegram = Some(kind);
                }
                _ => {}
            }
        }
        out
    }

    /// The web client of the Channel `kind`, if it reports one.
    pub(crate) fn client_of(d: &Deployment, kind: &str) -> Option<WebClient> {
        d.channel(kind)
            .and_then(|c| c.mini_app())
            .and_then(MiniApp::web_client)
    }
}
