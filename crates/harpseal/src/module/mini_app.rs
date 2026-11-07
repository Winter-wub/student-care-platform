//! The Mini App part of [`ModuleRegistrar`]: Mini App buttons a Module
//! sends, and its own Mini App pages linked from the home (#72). Kept
//! apart from `module.rs` so each file stays small.

use super::ModuleRegistrar;

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
