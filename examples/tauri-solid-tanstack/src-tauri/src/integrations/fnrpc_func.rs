use std::sync::atomic::{AtomicU64, Ordering};
use crate::ctx::Ctx;

#[fnrpc::rpc_query]
pub async fn health_check() -> &'static str {
    "ok"
}


static COUNTER: AtomicU64 = AtomicU64::new(0);

#[fnrpc::rpc_query]
pub async fn get_count() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("count: {n}")
}

fnrpc::fnrpc_registry! { Router<Ctx> {
    queries: [
      get_count,
      health_check,
      crate::feat::demo::func::greet,
      crate::feat::demo::func::add,
      crate::feat::demo::func::get_user,
      crate::feat::demo::func::divide,
    ],
    mutations: [
      crate::feat::demo::func::create_user,
    ],
    subscriptions: [
      crate::feat::demo::func::tick,
      crate::feat::demo::func::echo_stream,
      crate::feat::demo::func::watch_status,
    ],
} }
