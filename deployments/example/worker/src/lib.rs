//! STUB (prototype). The fictional Deployment as a Cloudflare Worker.
//! Will be: `#[event(fetch)]` -> `Arc<Deployment>::router()` via the axum
//! feature, `#[event(scheduled)]` -> due Jobs. Built with `worker-build`.

pub use example_deployment::DEPLOYMENT_TOML;
pub use harpseal_runtime_cloudflare::worker;
