//! The Mini App part of [`ModuleRegistrar`]: Mini App buttons a Module
//! sends, and its own Mini App pages linked from the home (#72); and the
//! Mini App URL of a page for a Job or a page to put in an email (#163).
//! Kept apart from `module.rs` so each file stays small.

use super::{JobContext, ModuleRegistrar, PageState};
use crate::{ChannelKind, Deployment};

/// A link on the Mini App home (#72), as registered.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AppLinkSpec {
    pub(crate) label_key: &'static str,
    pub(crate) path: &'static str,
    pub(crate) order: u16,
}

impl ModuleRegistrar {
    /// Declares that this Module sends `Button::MiniApp` to `path`.
    /// `build()` then requires every Channel to have a Mini App. A Mini App
    /// button sent without this declaration is not checked at build time:
    /// the adapter degrades it at send time to a visible notice.
    pub fn mini_app_button(&mut self, path: &'static str) -> &mut Self {
        self.mini_app_paths.push(path);
        self
    }

    /// A link to one of this Module's Mini App pages on the Mini App home
    /// (`/app`): `label_key` is a catalog key, `order` sorts the links of
    /// every Module, the Kernel's included (lower first; equal orders keep
    /// registration order). The page itself is a [`ModuleRegistrar::public`]
    /// route whose handler extracts [`crate::AppStudent`].
    ///
    /// `build()` refuses ([`crate::BuildError::InvalidRoute`]) a `path`
    /// that is not a plain path below `/app/` (not `/app/` itself; no
    /// parameters, query or `..`) or that this Module did not register with
    /// [`ModuleRegistrar::public`], and ([`crate::BuildError::UnknownText`])
    /// an unknown `label_key`. Only under `/app` does a page get the App
    /// Session cookie (`Path=/app`), the Mini App CSP and its deep link.
    pub fn mini_app_link(
        &mut self,
        label_key: &'static str,
        path: &'static str,
        order: u16,
    ) -> &mut Self {
        self.app_links.push(AppLinkSpec {
            label_key,
            path,
            order,
        });
        self
    }
}

impl Deployment {
    /// The URL that opens Mini App page `path` on the Channel of `kind`
    /// (#163): the very URL a [`crate::Button::mini_app`] to `path` opens
    /// there, built by the Channel's [`crate::MiniApp::launch_url`] (on
    /// LINE the LIFF or LINE MINI App URL, `https://liff.line.me/<id>/app/fees`;
    /// on Telegram the Web App URL followed by `path`). `path` is passed as
    /// a Mini App button's is, e.g. `/app/fees`.
    ///
    /// For a Job or a page that writes an email or a web page: a button in
    /// a chat message needs no URL. `None` when the Channel does not take
    /// traffic here ([`Deployment::channel`]) or has no Mini App
    /// configured.
    ///
    /// A LINE URL opens the page in LINE from anywhere (the app on a
    /// phone, LINE Login in a browser), so it works from an email. A
    /// Telegram Web App URL works only inside Telegram: opened in a
    /// browser it shows the Mini App's "open it from a button in the chat"
    /// page.
    pub fn mini_app_url(&self, kind: &str, path: &str) -> Option<String> {
        let app = self.channel(kind)?.mini_app()?;
        Some(app.launch_url(path))
    }

    /// [`Deployment::mini_app_url`] on every Channel that takes traffic
    /// and has a Mini App, in registration order: for an email to a
    /// Student who may use either chat app.
    pub fn mini_app_urls(&self, path: &str) -> Vec<(ChannelKind, String)> {
        self.channels()
            .filter_map(|c| Some((c.kind(), c.mini_app()?.launch_url(path))))
            .collect()
    }
}

impl JobContext {
    /// [`Deployment::mini_app_url`]: the URL that opens Mini App page
    /// `path` on the Channel of `kind`, e.g. for a reminder sent by email
    /// (#163).
    pub fn mini_app_url(&self, kind: &str, path: &str) -> Option<String> {
        self.deployment.mini_app_url(kind, path)
    }

    /// [`Deployment::mini_app_urls`]: the URL of Mini App page `path` on
    /// every Channel that has one.
    pub fn mini_app_urls(&self, path: &str) -> Vec<(ChannelKind, String)> {
        self.deployment.mini_app_urls(path)
    }
}

impl PageState {
    /// [`Deployment::mini_app_url`], from a page handler (#163).
    pub fn mini_app_url(&self, kind: &str, path: &str) -> Option<String> {
        self.deployment.mini_app_url(kind, path)
    }

    /// [`Deployment::mini_app_urls`], from a page handler.
    pub fn mini_app_urls(&self, path: &str) -> Vec<(ChannelKind, String)> {
        self.deployment.mini_app_urls(path)
    }
}
