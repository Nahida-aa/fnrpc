use fnrpc::router::RpcRouter;
use fnrpc::serializer::unpack_meta;
use serde_json::Value;
use tauri::ipc::Channel;

use crate::ctx::{AppState, Ctx};
use axum::http::HeaderMap;

#[tauri::command]
pub async fn rpc_fn(
    router: tauri::State<'_, RpcRouter<Ctx>>,
    state: tauri::State<'_, AppState>,
    path: String,
    input: Value,
) -> Result<Value, String> {
    let ctx = Ctx {
        state: state.inner().clone(),
        headers: HeaderMap::new(),
    };
    let input = unpack_meta(input);
    let input_bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
    let (result, _is_json) = router
        .dispatch(&ctx, &path, &input_bytes, false)
        .await
        .map_err(|e| serde_json::to_string(&e).unwrap())?;
    serde_json::from_slice(&result).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rpc_sub(
    router: tauri::State<'_, RpcRouter<Ctx>>,
    state: tauri::State<'_, AppState>,
    path: String,
    input: Value,
    channel: Channel<String>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let input = unpack_meta(input);
    let ctx = Ctx { state, headers: HeaderMap::new() };

    // For subscribe, we call dispatch and let the middleware chain handle it.
    // The subscribe handler is registered in the router's procedure metadata
    // but dispatch currently only handles query/mutate. For now, return an error.
    Err("subscribe not supported in this version".to_string())
}
