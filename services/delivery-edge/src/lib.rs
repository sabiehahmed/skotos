//! Skotos Flag Delivery Plane Edge (`services/delivery-edge`).
//!
//! Strictly read-only and DB-free on the hot path (ADR-0003): serves Server-Side
//! SDK SSE (`/stream/v1/all`), ETag pre-compressed polling (`/sdk/v1/latest-all`),
//! Client-Side Ping-to-Re-evaluate SSE (`/stream/v1/eval/{clientSideId}`), and
//! stateless `skotos-eval` evaluation (`/sdk/v1/evalx/...`).

pub mod auth;
pub mod cache;
pub mod routes;

pub use auth::AuthRegistry;
pub use cache::{CachedEnvData, EnvChannel, EnvironmentCache};
pub use routes::{build_router, ClientEvaluatedFlag, EdgeState};
