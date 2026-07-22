use crate::compare::utils::prebuild_get;
use dhat::{HeapStats, Profiler};
use fnrpc::RpcOutput;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::App;

// ── 变体 0:baseline(route_bytes + 借用 &'static [u8]) ──
// 当前 fnrpc-web/text 测量值:100B, 3 blocks/op
// 预期 3 个 block:
//   1. path String(lib.rs:291,dispatch key)
//   2. RawRpcFn::exec 的 Box::pin(bytes_handler.rs:80,宏生成)
//   3. BytesHandlerFn::call 的 Box::pin(router.rs:460,二次装箱)
// 响应体 b"ok" 走 Cow::Borrowed → Bytes::from_static,零拷贝,不分配。
#[fnrpc::rpc_bytes]
async fn text(_input: &[u8]) -> &'static [u8] {
    b"ok"
}

// ── 变体 2:route_raw(只装箱一次,无宏双重 Box::pin) ──
// 与 route_bytes 唯一区别:route_raw 的 OutputHandler::call 只 Box::pin 一次,
// 而 route_bytes 经过 RawRpcFn::exec(宏 Box::pin)+ BytesHandler::call(再 Box::pin)。
// 若 route_raw 比 route_bytes 少 1 block,则坐实「双重 Box::pin」假设。
// body 仍为借用 b"ok" → 零拷贝,排除 body 拷贝干扰。
#[fnrpc::rpc_raw]
async fn text_raw(_input: &[u8]) -> RpcOutput {
    RpcOutput::ok(b"ok")
}

// ── 变体 3:route_raw + 拥有型 body ──
// body 用 Vec<u8>(Cow::Owned)→ 响应体走 Bytes::copy_from_slice(lib.rs:425),
// 应在「变体 2」基础上 +1 block。若成立,坐实「借用 body 零拷贝」假设。
#[fnrpc::rpc_raw]
async fn text_raw_owned(_input: &[u8]) -> RpcOutput {
    RpcOutput::ok(b"ok".to_vec())
}

// ── 变体 4:空响应(排除 body 大小对 block 数的影响) ──
// 若空 body 仍是 3 blocks,说明 block 数与 body 内容无关,只与分发/期货路径有关。
#[fnrpc::rpc_bytes]
async fn text_empty(_input: &[u8]) -> &'static [u8] {
    b""
}

async fn run(label: &str, n: usize, build: fn() -> App<()>) {
    let app = build();
    let reqs = prebuild_get("/text", n);

    let _p = Profiler::builder()
        .file_name("benches/target/dhat-heap.json")
        .build();
    for req in reqs {
        let _ = app.call(req).await;
    }
    let s = HeapStats::get();
    eprintln!(
        "{}: {:>8}B, {:>6} blks  ({:>6.1}B, {:>5.1}blks/op)",
        label,
        s.total_bytes,
        s.total_blocks,
        s.total_bytes as f64 / n as f64,
        s.total_blocks as f64 / n as f64
    );
    drop(_p);
}

pub async fn bench_text(n: usize) {
    run("fnrpc-web/text", n, || {
        App::new(
            RpcRouterBuilder::<()>::new().route_bytes(text).build(),
            |_| (),
        )
    })
    .await;
}

/// route_raw + 借用 body:只装箱一次,用于对比 route_bytes 的双重 Box::pin。
pub async fn bench_text_route_raw(n: usize) {
    run("fnrpc-web/text-raw", n, || {
        App::new(
            RpcRouterBuilder::<()>::new().route_raw(text_raw).build(),
            |_| (),
        )
    })
    .await;
}

/// route_raw + 拥有型 body:验证借用 body 零拷贝、拥有 body +1 block。
pub async fn bench_text_raw_owned(n: usize) {
    run("fnrpc-web/text-raw-owned", n, || {
        App::new(
            RpcRouterBuilder::<()>::new()
                .route_raw(text_raw_owned)
                .build(),
            |_| (),
        )
    })
    .await;
}

/// 空响应:block 数应与 baseline 相同(3),证明 block 数与 body 内容无关。
pub async fn bench_text_empty(n: usize) {
    run("fnrpc-web/text-empty", n, || {
        App::new(
            RpcRouterBuilder::<()>::new()
                .route_bytes(text_empty)
                .build(),
            |_| (),
        )
    })
    .await;
}
