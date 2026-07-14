use crate::ctx::Ctx;

#[fnrpc::rpc_query]
pub async fn health_check() -> &'static str {
    "ok"
}

fnrpc::fnrpc_registry! { Router<Ctx> {
    queries: [
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
