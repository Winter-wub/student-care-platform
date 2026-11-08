//! Borrow-only access to the [`Deployment`] from a page handler or a Job
//! handler (#106), so a Module outside the Core can call the Core's
//! code-level services (`modules::announcements::create_draft` and
//! `confirm`, `roster::upload`, `texts::set_override`, …) with the same
//! Permission checks and audit rows as the Core's own pages.

use super::{JobContext, PageState};
use crate::Deployment;

impl PageState {
    /// The Deployment serving this request, for the Core's code-level
    /// services (see `docs/guide/deployment.md`, "Calling Core services
    /// from a Module page"). Pass the signed-in Staff member as
    /// [`crate::Actor::staff`] and the request's [`crate::RequestCtx`]
    /// (extract `ctx: RequestCtx`): each service checks that Staff
    /// member's Permission and audits the write as them, exactly as the
    /// Core page does.
    ///
    /// Use the borrow within the request. Do not clone the Deployment
    /// into anything that outlives it (a `static`, a Module field, a
    /// spawned task kept after the answer): the Deployment owns the
    /// registered handlers, so a copy kept by one of them is a reference
    /// cycle that is never freed.
    pub fn deployment(&self) -> &Deployment {
        &self.deployment
    }
}

impl JobContext {
    /// The Deployment running this Job, as [`PageState::deployment`]. A
    /// Job has no signed-in Staff member: a service that requires a
    /// Permission refuses `Actor::Job`, so a Job acting for Staff passes
    /// `Actor::Staff` with the id saved in its payload by the page that
    /// enqueued it, and that Staff member is checked again at run time.
    ///
    /// Use the borrow within the run. Never keep it, the context or a
    /// clone of either beyond the run: stored in a handler it would form
    /// a cycle (Deployment → registry → handler → Deployment) and leak.
    pub fn deployment(&self) -> &Deployment {
        &self.deployment
    }
}
