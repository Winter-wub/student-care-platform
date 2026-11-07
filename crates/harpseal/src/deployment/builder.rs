//! `Deployment::builder()`: collects the parts, runs every
//! `Module::register`, and refuses every misconfiguration with a
//! [`BuildError`] before anything runs.

use std::collections::HashMap;
use std::sync::Arc;

use super::routes::Routes;
use super::{Deployment, Registry, Route};
use crate::config::DeploymentConfig;
use crate::error::{BuildError, Part};
use crate::kernel::Kernel;
use crate::manifest::{Catalog, MenuEntrySpec, Permission};
use crate::module::NavSpec;
use crate::{Channel, Locale, Module, ModuleRegistrar, Services, VerificationMethod};

/// Assembles a [`Deployment`]: config, the runtime's [`Services`], Channels,
/// a Verification Method and Modules. Nothing is checked until
/// [`DeploymentBuilder::build`].
#[derive(Default)]
pub struct DeploymentBuilder {
    config: Option<DeploymentConfig>,
    services: Option<Services>,
    channels: Vec<Arc<dyn Channel>>,
    modules: Vec<Box<dyn Module>>,
    verification: Option<Arc<dyn VerificationMethod>>,
}

impl DeploymentBuilder {
    /// The parsed `deployment.toml`.
    pub fn config(mut self, c: DeploymentConfig) -> Self {
        self.config = Some(c);
        self
    }
    /// The runtime: `harpseal_runtime_native::services(..)` or
    /// `harpseal_runtime_cloudflare::services(..)`.
    pub fn runtime(mut self, s: Services) -> Self {
        self.services = Some(s);
        self
    }
    /// Adds a Channel. Kinds must be unique.
    pub fn channel(mut self, c: impl Channel + 'static) -> Self {
        self.channels.push(Arc::new(c));
        self
    }
    /// The Verification Method (e.g. `EmailOtp`).
    pub fn verification(mut self, v: impl VerificationMethod + 'static) -> Self {
        self.verification = Some(Arc::new(v));
        self
    }
    /// Adds a Module. The Kernel is always registered first, implicitly.
    pub fn module(mut self, m: impl Module) -> Self {
        self.modules.push(Box::new(m));
        self
    }

    /// Runs every `Module::register`, merges, validates.
    pub fn build(self) -> Result<Deployment, BuildError> {
        let config = self.config.ok_or(BuildError::Missing(Part::Config))?;
        // Public fields can bypass `from_toml`; the values reach the page head.
        config
            .branding
            .check()
            .map_err(|e| BuildError::InvalidConfig(e.to_string()))?;
        config
            .roster
            .check(&config.default_locale)
            .map_err(|e| BuildError::InvalidConfig(e.to_string()))?;
        config
            .backup
            .check()
            .map_err(|e| BuildError::InvalidConfig(e.to_string()))?;
        config
            .announcements
            .check()
            .map_err(|e| BuildError::InvalidConfig(e.to_string()))?;
        let services = self.services.ok_or(BuildError::Missing(Part::Runtime))?;
        let verification = self
            .verification
            .ok_or(BuildError::Missing(Part::Verification))?;
        if self.channels.is_empty() {
            return Err(BuildError::Missing(Part::Channel));
        }
        for (i, c) in self.channels.iter().enumerate() {
            if self.channels[..i].iter().any(|d| d.kind() == c.kind()) {
                return Err(BuildError::DuplicateChannel(c.kind()));
            }
        }
        let kernel: Box<dyn Module> = Box::new(Kernel);
        let mut collected = Collected::default();
        let mut names: Vec<&'static str> = Vec::new();
        for m in std::iter::once(&kernel).chain(self.modules.iter()) {
            if names.contains(&m.name()) {
                return Err(BuildError::DuplicateModule(m.name().into()));
            }
            names.push(m.name());
            let mut r = ModuleRegistrar::new(m.name());
            m.register(&mut r);
            if m.name() == crate::kernel::NAME {
                r.messages(crate::auth::TEXTS);
            }
            collected.absorb(r)?;
        }
        collected.check(&config, &self.channels)?;
        // The nightly backup (#78) runs on the TOML's `[backup] schedule`.
        if let Some(s) = crate::kernel::admin::backup_schedule(&config)? {
            collected.schedules.push(s);
        }
        let mut catalog = collected.catalog;
        check_toml_texts(&config, &catalog)?;
        config.apply_texts(&mut catalog);
        check_menu_texts(&collected.menu, &catalog)?;
        check_menu_keys(&catalog)?;
        check_nav(&collected.nav, &catalog)?;
        crate::module::check_settings(&collected.settings, &collected.permissions, &catalog)?;
        let mut nav = collected.nav;
        nav.extend(crate::kernel::admin::settings_nav(&collected.settings));
        let nav = nav_items(nav);
        let public: Vec<_> = collected.public.iter().map(|(m, p, _)| (*m, *p)).collect();
        let app_links = crate::kernel::app::check_links(collected.app_links, &public, &catalog)?;
        let mut menu: Vec<MenuEntrySpec> = collected.menu.into_iter().map(|(_, e)| e).collect();
        menu.sort_by_key(|e| e.order);
        let queue =
            crate::jobs::Jobs::new(collected.jobs.keys().copied().collect(), services.clone());
        Ok(Deployment {
            config: Arc::new(config),
            queue,
            services,
            channels: self.channels,
            verification,
            catalog: Arc::new(catalog),
            inbound_queues: Default::default(),
            registry: Arc::new(Registry {
                permissions: collected.permissions,
                menu,
                actions: collected.actions,
                action_prefixes: collected.action_prefixes,
                jobs: collected.jobs,
                schedules: collected.schedules,
                migrations: collected.migrations,
                back_office: collected.back_office,
                uploads: collected.uploads,
                public: collected.public,
                nav,
                app_links,
                settings: collected.settings.into(),
            }),
        })
    }
}

type BackOffice = Vec<(&'static str, &'static str, &'static str, Route)>;
type Actions = HashMap<crate::ActionId, (&'static str, Arc<dyn crate::ActionHandler>)>;
type Prefixes = Vec<(&'static str, &'static str, Arc<dyn crate::ActionHandler>)>;

/// Everything every Module registered, merged.
struct Collected {
    permissions: Vec<Permission>,
    menu: Vec<(&'static str, MenuEntrySpec)>,
    actions: Actions,
    action_prefixes: Prefixes,
    jobs: HashMap<&'static str, (crate::JobSpec, Arc<dyn crate::JobHandler>)>,
    schedules: Vec<(&'static str, crate::jobs::cron::Schedule)>,
    migrations: Vec<crate::Migration>,
    back_office: BackOffice,
    uploads: Vec<crate::module::UploadSpec>,
    public: Vec<(&'static str, &'static str, Route)>,
    routes: Routes,
    mini_app_users: Vec<&'static str>,
    catalog: Catalog,
    nav: Vec<(&'static str, NavSpec)>,
    app_links: Vec<(&'static str, crate::module::AppLinkSpec)>,
    settings: Vec<crate::module::SettingSpec>,
}

impl Default for Collected {
    fn default() -> Self {
        Self {
            permissions: Vec::new(),
            menu: Vec::new(),
            actions: HashMap::new(),
            action_prefixes: Vec::new(),
            jobs: HashMap::new(),
            schedules: Vec::new(),
            migrations: Vec::new(),
            back_office: Vec::new(),
            uploads: Vec::new(),
            public: Vec::new(),
            routes: Routes::new(),
            mini_app_users: Vec::new(),
            catalog: Catalog::default(),
            nav: Vec::new(),
            app_links: Vec::new(),
            settings: Vec::new(),
        }
    }
}

impl Collected {
    fn absorb(&mut self, r: ModuleRegistrar) -> Result<(), BuildError> {
        let module = r.module;
        for (id, h) in r.actions {
            if self.actions.contains_key(&id) {
                return Err(BuildError::DuplicateAction(id));
            }
            self.actions.insert(id, (module, h));
        }
        for (prefix, h) in r.action_prefixes {
            self.action_prefixes.push((prefix, module, h));
        }
        for (path, route) in r.public {
            self.routes.public(module, path)?;
            self.public.push((module, path, route));
        }
        for (permission, path, route) in r.back_office {
            self.routes.back_office(module, path)?;
            self.back_office.push((module, permission, path, route));
        }
        self.uploads.extend(r.uploads);
        for (spec, h) in r.jobs {
            if let Some(cron) = spec.schedule {
                let s = crate::jobs::cron::parse(cron).map_err(|reason| {
                    BuildError::InvalidSchedule {
                        kind: spec.kind.into(),
                        schedule: cron.into(),
                        reason,
                    }
                })?;
                self.schedules.push((spec.kind, s));
            }
            if self.jobs.insert(spec.kind, (spec, h)).is_some() {
                return Err(BuildError::DuplicateJob(spec.kind.into()));
            }
        }
        for p in r.permissions {
            if self.permissions.iter().any(|q| q.id == p.id) {
                return Err(BuildError::DuplicatePermission(p.id.into()));
            }
            self.permissions.push(p);
        }
        for e in r.catalog {
            self.catalog.insert(e.locale, e.key, e.text);
        }
        self.menu.extend(r.menu.into_iter().map(|e| (module, e)));
        self.nav.extend(r.nav.into_iter().map(|n| (module, n)));
        self.app_links
            .extend(r.app_links.into_iter().map(|l| (module, l)));
        crate::module::absorb_settings(&mut self.settings, r.settings)?;
        self.migrations.extend(r.migrations);
        if !r.mini_app_paths.is_empty() {
            self.mini_app_users.push(module);
        }
        Ok(())
    }

    /// Checks that need every Module's Permissions first.
    fn check(
        &self,
        config: &DeploymentConfig,
        channels: &[Arc<dyn Channel>],
    ) -> Result<(), BuildError> {
        let known = |id: &str| self.permissions.iter().any(|p| p.id == id);
        for (module, permission, _, _) in &self.back_office {
            if !known(permission) {
                return Err(BuildError::UnguardedRoute { module, permission });
            }
        }
        for (role, perms) in &config.roles {
            if let Some(p) = perms.iter().find(|p| !known(p)) {
                return Err(BuildError::UnknownPermission {
                    role: role.clone(),
                    permission: p.clone(),
                });
            }
        }
        self.check_prefixes()?;
        for module in &self.mini_app_users {
            if let Some(c) = channels.iter().find(|c| c.mini_app().is_none()) {
                return Err(BuildError::MiniAppUnavailable {
                    module,
                    channel: c.kind(),
                });
            }
        }
        Ok(())
    }
}

impl Collected {
    /// Action prefixes end with `.`, never overlap, and cover no exact id.
    fn check_prefixes(&self) -> Result<(), BuildError> {
        let refuse = |p: &str| {
            let id =
                crate::ActionId::new(p).map_err(|e| BuildError::InvalidConfig(e.to_string()))?;
            Err(BuildError::DuplicateAction(id))
        };
        for (i, (p, _, _)) in self.action_prefixes.iter().enumerate() {
            if !p.ends_with('.') {
                return refuse(p);
            }
            let overlaps = self.action_prefixes[..i]
                .iter()
                .any(|(q, _, _)| p.starts_with(q) || q.starts_with(p));
            if overlaps || self.actions.keys().any(|id| id.as_str().starts_with(p)) {
                return refuse(p);
            }
        }
        Ok(())
    }
}

/// Every Menu label must have a text in the default Locale (the fallback).
fn check_menu_texts(
    menu: &[(&'static str, MenuEntrySpec)],
    catalog: &Catalog,
) -> Result<(), BuildError> {
    let locale = catalog.default_locale();
    match menu
        .iter()
        .find(|(_, e)| !catalog.has(&locale, e.label_key))
    {
        Some((module, e)) => Err(BuildError::UnknownText {
            source: format!("module `{module}`"),
            key: e.label_key.into(),
        }),
        None => Ok(()),
    }
}

/// Every Locale of the catalog maps to its own Menu key, so the Kernel's
/// Menus (keyed by the effective Locale) always carry one.
fn check_menu_keys(catalog: &Catalog) -> Result<(), BuildError> {
    let mut seen: Vec<(crate::MenuKey, Locale)> = Vec::new();
    for locale in catalog.locales() {
        let bad = |reason: String| BuildError::MenuKey {
            locale: locale.0.clone(),
            reason,
        };
        let key = crate::MenuKey::for_locale(&locale).map_err(|e| bad(e.to_string()))?;
        if let Some((_, other)) = seen.iter().find(|(k, _)| *k == key) {
            return Err(bad(format!(
                "`{}` is also the key of Locale `{}`",
                key.as_str(),
                other.0
            )));
        }
        seen.push((key, locale));
    }
    Ok(())
}

/// Navigation entries: a known title in the default Locale, no parameters.
fn check_nav(nav: &[(&'static str, NavSpec)], catalog: &Catalog) -> Result<(), BuildError> {
    let locale = catalog.default_locale();
    for (module, n) in nav {
        if n.path.contains('{') {
            return Err(BuildError::InvalidRoute {
                module,
                path: n.path.into(),
            });
        }
        if !catalog.has(&locale, n.title_key) {
            return Err(BuildError::UnknownText {
                source: format!("module `{module}`"),
                key: n.title_key.into(),
            });
        }
    }
    Ok(())
}

fn nav_items(nav: Vec<(&'static str, NavSpec)>) -> Vec<crate::auth::NavItem> {
    let mut items: Vec<crate::auth::NavItem> = nav
        .into_iter()
        .map(|(_, n)| crate::auth::NavItem {
            permission: n.permission,
            path: format!("{}{}", super::routes::ADMIN, n.path),
            title_key: n.title_key,
            order: n.order,
        })
        .collect();
    items.sort_by_key(|n| n.order);
    items
}

/// `[texts]` may only override keys some Module declares: a typo would
/// otherwise be ignored silently.
fn check_toml_texts(config: &DeploymentConfig, catalog: &Catalog) -> Result<(), BuildError> {
    for (locale, entries) in &config.texts {
        if let Some(key) = entries.keys().find(|k| !catalog.knows(k)) {
            return Err(BuildError::UnknownText {
                source: format!("deployment.toml [texts.{locale}]"),
                key: key.clone(),
            });
        }
    }
    Ok(())
}
