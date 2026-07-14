use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use fnrpc::router::RpcRouter;
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

async fn rpc_fn_axum(
    method: Method,
    State(state): State<AxumState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    body: Option<axum::extract::Json<Value>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let ctx = Ctx {
        state: state.app_state,
        headers,
    };

    let input = match method {
        Method::GET => {
            let raw = params.get("input").cloned().unwrap_or_else(|| "null".into());
            serde_json::from_str(&raw).unwrap_or(Value::Null)
        }
        Method::POST => body.map(|j| j.0).unwrap_or(Value::Null),
        _ => Value::Null,
    };

    match state.router.dispatch(&ctx, &path, input).await {
        Ok(val) => Ok(Json(val)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn rpc_sub_axum(
    State(state): State<AxumState>,
    Path(path): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Sse<ReceiverStream<Result<Event, Infallible>>> {
    let handler = state
        .router
        .get_sub_handler(&path)
        .expect("unknown subscription path");

    let raw = params.get("input").cloned().unwrap_or_else(|| "null".into());
    let input: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    let ctx = Ctx {
        state: state.app_state,
        headers: HeaderMap::new(),
    };

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
        let mut stream = handler.call(&ctx, input);
        while let Some(item) = stream.next().await {
            match item {
                Ok(val) => {
                    let data = match &val {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let _ = tx.send(Ok(Event::default().data(data))).await;
                }
                Err(_) => break,
            }
        }
    });

    Sse::new(ReceiverStream::new(rx))
}

pub fn build_axum_router(router: RpcRouter<Ctx>, app_state: AppState) -> Router {
    let cors = CorsLayer::permissive();

    let state = AxumState {
        router: Arc::new(router),
        app_state,
    };

    Router::new()
        .route("/fnrpc/{*path}", get(rpc_fn_axum).post(rpc_fn_axum))
        .route("/fnrpc/sub/{*path}", get(rpc_sub_axum))
        .layer(cors)
        .with_state(state)
}
