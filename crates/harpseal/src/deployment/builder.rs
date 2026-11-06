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
use crate::{Channel, Module, ModuleRegistrar, Services, VerificationMethod};

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
            collected.absorb(r)?;
        }
        collected.check(&config, &self.channels)?;
        let mut catalog = collected.catalog;
        check_toml_texts(&config, &catalog)?;
        config.apply_texts(&mut catalog);
        check_menu_texts(&collected.menu, &catalog)?;
        let mut menu: Vec<MenuEntrySpec> = collected.menu.into_iter().map(|(_, e)| e).collect();
        menu.sort_by_key(|e| e.order);
        Ok(Deployment {
            config: Arc::new(config),
            services,
            channels: self.channels,
            verification,
            catalog: Arc::new(catalog),
            registry: Registry {
                permissions: collected.permissions,
                menu,
                actions: collected.actions,
                jobs: collected.jobs,
                migrations: collected.migrations,
                back_office: collected.back_office,
                public: collected.public,
            },
        })
    }
}

type BackOffice = Vec<(&'static str, &'static str, &'static str, Route)>;
type Actions = HashMap<crate::ActionId, (&'static str, Arc<dyn crate::ActionHandler>)>;

/// Everything every Module registered, merged.
struct Collected {
    permissions: Vec<Permission>,
    menu: Vec<(&'static str, MenuEntrySpec)>,
    actions: Actions,
    jobs: HashMap<&'static str, (crate::JobSpec, Arc<dyn crate::JobHandler>)>,
    migrations: Vec<crate::Migration>,
    back_office: BackOffice,
    public: Vec<(&'static str, &'static str, Route)>,
    routes: Routes,
    mini_app_users: Vec<&'static str>,
    catalog: Catalog,
}

impl Default for Collected {
    fn default() -> Self {
        Self {
            permissions: Vec::new(),
            menu: Vec::new(),
            actions: HashMap::new(),
            jobs: HashMap::new(),
            migrations: Vec::new(),
            back_office: Vec::new(),
            public: Vec::new(),
            routes: Routes::new(),
            mini_app_users: Vec::new(),
            catalog: Catalog::default(),
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
        for (path, route) in r.public {
            self.routes.public(module, path)?;
            self.public.push((module, path, route));
        }
        for (permission, path, route) in r.back_office {
            self.routes.back_office(module, path)?;
            self.back_office.push((module, permission, path, route));
        }
        for (spec, h) in r.jobs {
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
