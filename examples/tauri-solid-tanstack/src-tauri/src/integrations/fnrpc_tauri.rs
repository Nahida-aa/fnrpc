use fnrpc::router::RpcRouter;
use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
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

    let input_bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
    let mut stream = router
        .dispatch_subscribe(&ctx, &path, &input_bytes)
        .map_err(|e| serde_json::to_string(&e).unwrap())?;

    tauri::async_runtime::spawn(async move {
        while let Some(item) = stream.next().await {
            match item {
                Ok(bytes) => {
                    if let Ok(s) = String::from_utf8(bytes.into_owned()) {
                        // When the client disconnects, the JS side clears
                        // channel.onmessage, allowing the channel to be GC'd.
                        // Tauri then drops the Rust-side channel handle,
                        // making send() return Err.
                        if channel.send(s).is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    let _ = channel.send(serde_json::to_string(&e).unwrap());
                    break;
                }
            }
        }
    });

    Ok(())
}
