pub mod codec;
pub mod error;
pub mod gen_ts_client;
pub mod handler;
pub mod middleware;
pub mod middlewares;
pub mod output;
pub mod router;
pub mod serializer;

/// Framework-agnostic handler output (payload + optional HTTP status/headers).
///
/// Returned by [`route_raw`](crate::router::RpcRouterBuilder::route_raw) handlers.
pub use output::{HttpInfo, RpcOutput};

/// Convenience re-exports for common middleware traits.
pub mod prelude {
    pub use crate::middleware::{NextExt, RpcService, ServiceExt};
    pub use crate::middlewares::hook::HookLayer;
}

pub use fnrpc_macros::{rpc_bytes, rpc_mutate, rpc_query, rpc_raw, rpc_subscribe};
