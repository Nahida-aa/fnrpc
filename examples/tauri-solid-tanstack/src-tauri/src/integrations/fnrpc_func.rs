use crate::{ctx::Ctx, feat::demo::func::post_echo_stream};
use fnrpc::middlewares::tracing::TracingLayer;
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

pub fn build_fn_rpc_router() -> fnrpc::router::RpcRouter<Ctx> {
    fnrpc::router::RpcRouterBuilder::<Ctx>::new()
        .route_fn(get_count)
        .route_fn(reset_count)
        .route_fn(health_check)
        .route_fn(crate::feat::demo::func::greet)
        .route_fn(crate::feat::demo::func::add)
        .route_fn(crate::feat::demo::func::get_user)
        .route_fn(crate::feat::demo::func::divide)
        .route_fn(crate::feat::demo::func::create_user)
        .subscribe(crate::feat::demo::func::tick)
        .subscribe(crate::feat::demo::func::echo_stream)
        .subscribe(post_echo_stream)
        .subscribe(crate::feat::demo::func::watch_status)
        .layer(TracingLayer)
        .build()
}
