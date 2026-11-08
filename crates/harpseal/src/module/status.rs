//! The back-office navigation groups and the home page's status cards
//! (#123): which group a page is listed under, and what a Module adds to
//! the term status home at `/admin`.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use axum::routing::MethodRouter;

use super::{ModuleRegistrar, NavSpec};
use crate::{AppState, CurrentStaff, Result};

/// The navigation group a back-office page is listed under.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum NavGroup {
    /// "งานประจำ": the recurring work (Students, the roster, announcements,
    /// fees, grades). The default for [`super::ModuleRegistrar::back_office_page`].
    #[default]
    Daily,
    /// "ผู้ดูแลระบบ": Staff, Roles, texts, settings, branding, usage, the
    /// audit log and backups.
    Admin,
}

/// How a status card reads at a glance. Its colour is fixed, never the
/// brand: neutral, approved or caution (DESIGN.md "Status card").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CardState {
    /// Nothing has happened yet: the fact says what to do first
    /// ("ยังไม่ได้นำเข้ารายชื่อ") and the action starts it.
    Empty,
    /// In order.
    Ok,
    /// Needs a Staff member's attention (a failed import, a quota near
    /// its cap, Students who still owe fees).
    Warn,
}

impl CardState {
    /// `empty`, `ok` or `warn`: the card's `data-state`.
    pub fn as_str(self) -> &'static str {
        match self {
            CardState::Empty => "empty",
            CardState::Ok => "ok",
            CardState::Warn => "warn",
        }
    }
}

/// What a status card says on the term status home: a state, one fact
/// and one next action, already in the Staff member's Locale. The title
/// is the card's `title_key` ([`ModuleRegistrar::status_card`]), so even a
/// card that failed to read is named.
///
/// ```
/// use harpseal::{CardState, StatusCard};
/// let card = StatusCard::new(CardState::Warn, "12 คนค้างชำระ")
///     .action("ดูยอดค้างชำระ", "/admin/fees");
/// assert_eq!(card.state(), CardState::Warn);
/// assert_eq!(card.fact(), "12 คนค้างชำระ");
/// assert_eq!(card.action_link(), Some(("ดูยอดค้างชำระ", "/admin/fees")));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusCard {
    pub(crate) state: CardState,
    pub(crate) fact: String,
    pub(crate) action: Option<(String, String)>,
}

impl StatusCard {
    /// A card with no action yet.
    pub fn new(state: CardState, fact: impl Into<String>) -> Self {
        Self {
            state,
            fact: fact.into(),
            action: None,
        }
    }

    /// The one next action: a link labelled `label` to `href` (a
    /// back-office path the Staff member may open).
    pub fn action(mut self, label: impl Into<String>, href: impl Into<String>) -> Self {
        self.action = Some((label.into(), href.into()));
        self
    }

    /// The card's state.
    pub fn state(&self) -> CardState {
        self.state
    }

    /// The card's fact.
    pub fn fact(&self) -> &str {
        &self.fact
    }

    /// The next action as `(label, href)`, if one was set.
    pub fn action_link(&self) -> Option<(&str, &str)> {
        self.action.as_ref().map(|(l, h)| (l.as_str(), h.as_str()))
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Builds a status card for the signed-in Staff member: an
/// `async fn(AppState, CurrentStaff) -> harpseal::Result<StatusCard>` or
/// an equivalent closure (the only implementations; the trait is sealed).
/// Runs only on the home page, only for Staff holding the card's
/// Permission, once per view: keep it to one or two queries. An `Err`
/// shows the card's title with "could not read"; the rest of the home
/// still renders.
#[async_trait]
pub trait StatusSource: sealed::Sealed + Send + Sync {
    /// The card.
    async fn card(&self, s: AppState, staff: CurrentStaff) -> Result<StatusCard>;
}

impl<F, Fut> sealed::Sealed for F
where
    F: Fn(AppState, CurrentStaff) -> Fut + Send + Sync,
    Fut: Future<Output = Result<StatusCard>> + Send + 'static,
{
}

#[async_trait]
impl<F, Fut> StatusSource for F
where
    F: Fn(AppState, CurrentStaff) -> Fut + Send + Sync,
    Fut: Future<Output = Result<StatusCard>> + Send + 'static,
{
    async fn card(&self, s: AppState, staff: CurrentStaff) -> Result<StatusCard> {
        (self)(s, staff).await
    }
}

/// A registered status card.
#[derive(Clone)]
pub(crate) struct StatusSpec {
    pub(crate) permission: &'static str,
    pub(crate) title_key: &'static str,
    pub(crate) order: u16,
    pub(crate) source: Arc<dyn StatusSource>,
}

impl ModuleRegistrar {
    /// [`ModuleRegistrar::back_office_page`] listed under `group` in the
    /// navigation: [`NavGroup::Admin`] for an administration page (Staff,
    /// settings, technical tools). Groups and entries show only what the
    /// Staff member's Permissions allow; within a group entries sort by
    /// `nav_order`.
    pub fn back_office_page_in(
        &mut self,
        group: NavGroup,
        permission: &'static str,
        path: &'static str,
        title_key: &'static str,
        nav_order: u16,
        route: MethodRouter<AppState>,
    ) -> &mut Self {
        self.nav.push(NavSpec {
            permission,
            path,
            title_key,
            order: nav_order,
            group,
        });
        self.back_office(permission, path, route)
    }

    /// A status card on the term status home (`/admin`), shown only to
    /// Staff holding `permission` and rendered by the Core shell with
    /// every other card, sorted by `order` (lower first; the Kernel's
    /// cards use 10 to 40). `title_key` is the catalog key of its title,
    /// shown even when `source` fails. `source` runs once per home view
    /// and never on other pages, so a card costs at most its own queries
    /// there: keep it to one or two. `build()` refuses an undeclared
    /// `permission` ([`crate::BuildError::UnguardedStatusCard`]) and a
    /// `title_key` with no text in the default Locale
    /// ([`crate::BuildError::UnknownText`]).
    ///
    /// ```no_run
    /// use harpseal::{AppState, CardState, CurrentStaff, ModuleRegistrar, StatusCard};
    ///
    /// async fn fees(s: AppState, staff: CurrentStaff) -> harpseal::Result<StatusCard> {
    ///     let owing = 12; // one COUNT query in a real card
    ///     let fact = s.catalog.tf(&staff.locale, "fees.home.owing", &[("n", &owing.to_string())]);
    ///     Ok(StatusCard::new(CardState::Warn, fact).action("ดูยอดค้างชำระ", "/admin/fees"))
    /// }
    ///
    /// fn register(r: &mut ModuleRegistrar) {
    ///     r.status_card("fees.import", "fees.home.title", 50, fees);
    /// }
    /// ```
    pub fn status_card(
        &mut self,
        permission: &'static str,
        title_key: &'static str,
        order: u16,
        source: impl StatusSource + 'static,
    ) -> &mut Self {
        self.status.push(StatusSpec {
            permission,
            title_key,
            order,
            source: Arc::new(source),
        });
        self
    }
}
