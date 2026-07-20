use std::collections::HashMap;
use std::sync::Mutex;

use fnrpc::router::RpcRouter;
use fnrpc::serializer::unpack_meta;
use futures::StreamExt;
use serde_json::Value;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::ctx::{AppState, Ctx};
use axum::http::HeaderMap;

/// Tracks active subscriptions so they can be cancelled on client disconnect.
static ACTIVE_SUBS: std::sync::LazyLock<Mutex<HashMap<u32, CancellationToken>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

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

    let cancel = CancellationToken::new();
    ACTIVE_SUBS.lock().unwrap().insert(channel.id(), cancel.clone());

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                item = stream.next() => {
                    match item {
                        Some(Ok(bytes)) => {
                            if let Ok(s) = String::from_utf8(bytes.into_owned()) {
                                if channel.send(s).is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = channel.send(serde_json::to_string(&e).unwrap());
                            break;
                        }
                        None => break,
                    }
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }
        ACTIVE_SUBS.lock().unwrap().remove(&channel.id());
    });

    Ok(())
}

/// Cancel a subscription by channel ID. Called from the JS side when the
/// client cancels the async iterator (e.g. via consumeEventIterator's cancel).
#[tauri::command]
pub async fn rpc_cancel_sub(channel_id: u32) -> Result<(), String> {
    if let Some(cancel) = ACTIVE_SUBS.lock().unwrap().remove(&channel_id) {
        cancel.cancel();
    }
    Ok(())
}
