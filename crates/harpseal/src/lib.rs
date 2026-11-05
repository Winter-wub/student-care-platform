//! harpseal — the Core. THROWAWAY PROTOTYPE for issue #4.
//!
//! Shared vocabulary (message model, ids, Db value types, Module manifest
//! types, TOML config) lives at the crate root and is used by both API
//! designs. `design_a` (trait objects + runtime registry) and `design_b`
//! (generics + static dispatch) are two alternative public APIs over the
//! same vocabulary, kept side by side so they can be compared.

pub mod config;
pub mod delivery;
pub mod error;
pub mod manifest;
pub mod message;
pub mod port_types;
pub mod verification;

pub mod design_a;
pub mod design_b;

pub use error::{BuildError, Error, Result};

pub(crate) mod kernel;
