use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use fnrpc::router::RpcRouter;
use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

use crate::ctx::{AppState, Ctx};

#[derive(Clone)]
struct AxumState {
    router: Arc<RpcRouter<Ctx>>,
    app_state: AppState,
}

async fn fnrpc_handle(
    method: Method,
    State(state): State<AxumState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    body: Option<axum::extract::Json<Value>>,
) -> axum::response::Response {
    let kind = state.router.get_procedure_kind(&path);

    match kind {
        Some("subscribe") => {
            let raw = params.get("input").cloned().unwrap_or_else(|| "null".into());
            let input_raw: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            let input = unpack_meta(&input_raw);

            match state.router.get_sub_handler(&path) {
                Some(handler) => {
                    let ctx = Ctx {
                        state: state.app_state,
                        headers,
                    };

                    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

                    tokio::spawn(async move {
                        let mut stream = handler.call(&ctx, input);
                        while let Some(item) = stream.next().await {
                            let event = match item {
                                Ok(val) => Event::default().json_data(val).unwrap(),
                                Err(e) => Event::default().data(format!("__error:{}", e)),
                            };
                            if tx.send(Ok(event)).await.is_err() {
                                break;
                            }
                        }
                    });

                    Sse::new(ReceiverStream::new(rx))
                        .keep_alive(KeepAlive::default())
                        .into_response()
                }
                None => (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("unknown path: {path}") })),
                )
                    .into_response(),
            }
        }
        Some(_) => {
            // query or mutate
            let ctx = Ctx {
                state: state.app_state,
                headers,
            };

            let input_raw = match method {
                Method::GET => {
                    let raw = params.get("input").cloned().unwrap_or_else(|| "null".into());
                    serde_json::from_str(&raw).unwrap_or(Value::Null)
                }
                Method::POST => body.map(|j| j.0).unwrap_or(Value::Null),
                _ => Value::Null,
            };
            let input = unpack_meta(&input_raw);

            match state.router.dispatch(&ctx, &path, input).await {
                Ok(val) => Json(val).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("unknown path: {path}") })),
        )
            .into_response(),
    }
}

pub fn build_axum_router(router: RpcRouter<Ctx>, app_state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    let state = AxumState {
        router: Arc::new(router),
        app_state,
    };

    Router::new()
        .route("/fnrpc/{*path}", get(fnrpc_handle).post(fnrpc_handle))
        .layer(cors)
        .with_state(state)
}
