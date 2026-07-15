use crate::ctx::Ctx;
use fnrpc::error::RpcErr;
use serde::{Deserialize, Serialize};
use specta::Type;

#[fnrpc::rpc_query]
pub async fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

#[fnrpc::rpc_query]
pub async fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct User {
    pub id: u32,
    pub name: String,
    pub email: String,
}

#[fnrpc::rpc_query]
pub async fn get_user(id: u32) -> User {
    User {
        id,
        name: "Alice".into(),
        email: "alice@example.com".into(),
    }
}

#[fnrpc::rpc_query]
pub async fn divide(a: f64, b: f64) -> Result<f64, RpcErr> {
    if b == 0.0 {
        Err(RpcErr::bad_request("cannot divide by zero"))
    } else {
        Ok(a / b)
    }
}

#[fnrpc::rpc_mutate]
pub async fn create_user(_ctx: &Ctx, name: String, email: String) -> User {
    User {
        id: 42,
        name,
        email,
    }
}

#[fnrpc::rpc_subscribe]
pub fn tick(interval_ms: u64) -> impl futures::Stream<Item = u64> {
    futures::stream::unfold(0u64, move |count| async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
        Some((count, count + 1))
    })
}

#[fnrpc::rpc_subscribe]
pub fn echo_stream(prefix: String) -> impl futures::Stream<Item = String> {
    futures::stream::unfold(0u64, move |count| {
        let prefix = prefix.clone();
        async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            Some((format!("{prefix} #{count}"), count + 1))
        }
    })
}

#[fnrpc::rpc_subscribe]
pub fn watch_status(ctx: &Ctx, key: String) -> impl futures::Stream<Item = String> {
    let app_dir = ctx.state.app_dir.clone();
    futures::stream::unfold((0u64, key, app_dir), |(count, key, app_dir)| async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let msg = format!("[{key}] tick #{count} (dir: {})", app_dir.display());
        Some((msg, (count + 1, key, app_dir)))
    })
}
