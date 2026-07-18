//! xitca-web integration for fnrpc.
//! Placeholder — being refactored for zero-erasure architecture.

use std::sync::Arc;
use xitca_web::http::header::HeaderMap;

/// Placeholder state.
pub struct FnrpcState<Ctx> {
    pub ctx_from_headers: Arc<dyn Fn(&HeaderMap) -> Ctx + Send + Sync>,
}
