//! Which platform script the entry page needs for which Channel. The
//! `MiniApp` Capability only gives launch URLs, so the Kernel recognises
//! LIFF by its launch URL (`https://liff.line.me/{liffId}`, whose id
//! `liff.init` needs) and the Telegram Web App by the Channel kind
//! `telegram`. A Channel that is neither gets no script (the Fake
//! Channel: tests post the token themselves).

use crate::Deployment;

const LIFF_BASE: &str = "https://liff.line.me/";

/// The Mini App platforms of a Deployment.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sdks {
    /// (Channel kind, LIFF id).
    pub(crate) liff: Option<(String, String)>,
    /// Channel kind of the Telegram Web App.
    pub(crate) telegram: Option<String>,
}

impl Sdks {
    pub(crate) fn of(d: &Deployment) -> Self {
        let mut out = Self::default();
        for kind in d.channel_kinds() {
            let Some(app) = d.channel(kind.as_str()).and_then(|c| c.mini_app()) else {
                continue;
            };
            let launch = app.launch_url("");
            if let Some(id) = launch.strip_prefix(LIFF_BASE) {
                let id = id.trim_end_matches('/');
                if out.liff.is_none() && !id.is_empty() {
                    out.liff = Some((kind.as_str().to_string(), id.to_string()));
                }
            } else if kind.as_str() == "telegram" {
                out.telegram = Some(kind.as_str().to_string());
            }
        }
        out
    }
}
