use fnrpc::router::RpcRouter;
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
    router
        .dispatch(&ctx, &path, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rpc_sub(
    router: tauri::State<'_, RpcRouter<Ctx>>,
    state: tauri::State<'_, AppState>,
    path: String,
    input: Value,
    channel: Channel<String>,
) -> Result<(), String> {
    let handler = router
        .get_sub_handler(&path)
        .ok_or_else(|| format!("unknown subscribe path: {path}"))?;
    let state = state.inner().clone();

    tokio::spawn(async move {
        let ctx = Ctx { state, headers: HeaderMap::new() };
        let mut stream = handler.call(&ctx, input);
        while let Some(item) = stream.next().await {
            match item {
                Ok(val) => {
                    let s = match &val {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    if channel.send(s).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = channel.send(format!("__error:{}", e));
                    break;
                }
            }
        }
    });

    Ok(())
}
