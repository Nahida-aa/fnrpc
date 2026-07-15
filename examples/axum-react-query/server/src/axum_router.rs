use axum::Router;
use fnrpc::router::RpcRouter;
use fnrpc_axum::{handle, FnrpcState};
use tower_http::cors::CorsLayer;

use crate::ctx::{AppState, Ctx};

pub fn build_axum_router(router: RpcRouter<Ctx>, app_state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    Router::new()
        .route(
            "/fnrpc/{*path}",
            axum::routing::get(handle::<Ctx>).post(handle::<Ctx>),
        )
        .with_state(std::sync::Arc::new(FnrpcState {
            router: std::sync::Arc::new(router),
            ctx_from_headers: std::sync::Arc::new(move |headers| Ctx {
                state: app_state.clone(),
                headers,
            }),
        }))
        .layer(cors)
}
