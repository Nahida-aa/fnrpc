use crate::ctx::Ctx;
use std::sync::atomic::{AtomicU64, Ordering};

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

#[fnrpc::rpc_mutate]
pub async fn reset_count() -> () {
    COUNTER.store(0, Ordering::Relaxed);
}

// fnrpc::fnrpc_registry! { Router<Ctx> {
//     queries: [
//       get_count,
//       health_check,
//       crate::feat::demo::func::greet,
//       crate::feat::demo::func::add,
//       crate::feat::demo::func::get_user,
//       crate::feat::demo::func::divide,
//     ],
//     mutates: [
//       reset_count,
//       crate::feat::demo::func::create_user,
//     ],
//     subscribes: [
//       crate::feat::demo::func::tick,
//       crate::feat::demo::func::echo_stream,
//       crate::feat::demo::func::watch_status,
//     ],
// } }

/// fnrpc::fnrpc_registry! { Router<Ctx> {
///     queries: [
///       get_count,
///       health_check,
///       crate::feat::demo::func::greet,
///       crate::feat::demo::func::add,
///       crate::feat::demo::func::get_user,
///       crate::feat::demo::func::divide,
///     ],
///     mutates: [
///       reset_count,
///       crate::feat::demo::func::create_user,
///     ],
///     subscribes: [
///       crate::feat::demo::func::tick,
///       crate::feat::demo::func::echo_stream,
///       crate::feat::demo::func::watch_status,
///     ],
/// } }
///
pub fn build_fn_rpc_router() -> fnrpc::router::RpcRouter<Ctx> {
    fnrpc::router::RpcRouter::<Ctx>::new()
        .query(get_count)
        .mutate(reset_count)
        .query(health_check)
        .query(crate::feat::demo::func::greet)
        .query(crate::feat::demo::func::add)
        .query(crate::feat::demo::func::get_user)
        .query(crate::feat::demo::func::divide)
        .mutate(crate::feat::demo::func::create_user)
        .subscribe(crate::feat::demo::func::tick)
        .subscribe(crate::feat::demo::func::echo_stream)
        .subscribe(crate::feat::demo::func::watch_status)
}
