use criterion::{criterion_group, criterion_main, Criterion};

use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    async fn exec(_ctx: &(), _input: ()) -> Result<(), RpcErr> {
        Ok(())
    }
}

struct Echo;
impl RpcFn<()> for Echo {
    type Input = String;
    type Output = String;
    const NAME: &'static str = "echo";
    async fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
        Ok(input)
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_noop_query(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let router = RpcRouterBuilder::<()>::new().query(Noop).build();

    c.bench_function("handler/noop/query", |b| {
        b.to_async(&rt).iter(|| async {
            router
                .dispatch_send(&(), "noop", Value::Null)
                .await
                .unwrap();
        })
    });
}

fn bench_noop_mutate(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let router = RpcRouterBuilder::<()>::new().mutate(Noop).build();

    c.bench_function("handler/noop/mutate", |b| {
        b.to_async(&rt).iter(|| async {
            router
                .dispatch_send(&(), "noop", Value::Null)
                .await
                .unwrap();
        })
    });
}

fn bench_echo_string(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let router = RpcRouterBuilder::<()>::new().query(Echo).build();
    let input = serde_json::json!("hello benchmark");

    c.bench_function("handler/echo/string", |b| {
        b.to_async(&rt).iter(|| async {
            router
                .dispatch_send(&(), "echo", input.clone())
                .await
                .unwrap();
        })
    });
}

fn bench_not_found(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let router = RpcRouterBuilder::<()>::new().build();

    c.bench_function("handler/not_found", |b| {
        b.to_async(&rt).iter(|| async {
            router
                .dispatch_send(&(), "nonexistent", Value::Null)
                .await
                .unwrap_err();
        })
    });
}

criterion_group!(
    dispatch,
    bench_noop_query,
    bench_noop_mutate,
    bench_echo_string,
    bench_not_found,
);
criterion_main!(dispatch);
