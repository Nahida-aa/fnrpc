//! Framework-free comparison of two *handler-shape* paradigms.
//!
//! This deliberately imports **no web framework** (not fnrpc, not axum, not
//! xitca). We are not comparing frameworks — we are comparing how a handler's
//! "shape" (how its input is obtained and its output is produced) is expressed
//! and dispatched:
//!
//! 1. **Associated-item shape** (what fnrpc's proc-macro produces): the macro
//!    turns `fn echo(input: String) -> String` into `struct Echo; impl RpcFn
//!    for Echo { const KEY: &str; type Input; type Output; fn call(&self,
//!    &[u8]) -> Vec<u8>; }`. The shape lives on *trait associated items*. At
//!    registration the handler is erased into `Box<dyn DynHandler>` and stored
//!    in a table keyed by `KEY`. Dispatch looks the entry up and calls it
//!    directly on raw bytes — no per-request extraction trait is run.
//!
//! 2. **Signature-bound shape** (what axum's handler model is): the handler is
//!    a plain `fn(In) -> Out`. How bytes become `In` and how `Out` becomes
//!    bytes is decided by *trait bounds on the signature* (`FromRequest` /
//!    `IntoResponse`). At registration it is also erased into `Box<dyn
//!    DynHandler>`, but that dyn handler internally runs `In::extract(bytes)
//!    -> f -> Out::respond()` — two trait calls per request that the
//!    associated-item shape does not have (there, the handler owns its own
//!    deserialization inside `call`).
//!
//! Both sides use the *same* `Vec<(&str, Box<dyn DynHandler>)>` table, so the
//! only variable is the per-request work each shape implies. No HTTP, no
//! framework, no fnrpc crate.
//!
//! Run:
//!   cargo bench -p benches --bench routing_paradigm
//!
//! Note on what this proves: at the CPU level the two are nearly identical
//! once the bound-side trait calls are monomorphized. The *real* cost of the
//! signature-bound shape shows up as **per-request allocation** when the
//! extract/respond steps actually parse bytes into a structured type and
//! serialize it back (see the `real_extract` variants). That allocation gap
//! is the genuine paradigm difference; the ns difference alone is noise.

use criterion::{criterion_group, criterion_main, Criterion};

// ─────────────────────────────────────────────────────────────────────────────
// Shared byte-level plumbing
// ─────────────────────────────────────────────────────────────────────────────

type Bytes = Vec<u8>;

// A minimal routing table: path -> erased handler. (A `Vec` is enough; we are
// not measuring tree lookup — both paradigms share this exact table.)
struct Table {
    routes: Vec<(&'static str, Box<dyn DynHandler>)>,
}
impl Table {
    fn insert(&mut self, key: &'static str, h: Box<dyn DynHandler>) {
        self.routes.push((key, h));
    }
    fn dispatch(&self, path: &str, input: &[u8]) -> Bytes {
        for (k, h) in &self.routes {
            if *k == path {
                return h.handle(input);
            }
        }
        Vec::new()
    }
}

trait DynHandler {
    fn handle(&self, input: &[u8]) -> Bytes;
}

// ─────────────────────────────────────────────────────────────────────────────
// Paradigm A: associated-item shape  (fnrpc's proc-macro output, minus fnrpc)
// ─────────────────────────────────────────────────────────────────────────────

/// Mirrors the `RpcFn` contract the macro generates: KEY + Input/Output types
/// are associated items; `call` takes raw bytes and returns raw bytes, owning
/// its own (de)serialization. No extraction trait is executed at dispatch.
trait RpcFn {
    const KEY: &'static str;
    fn call(&self, input: &[u8]) -> Bytes;
}
impl<T: RpcFn> DynHandler for T {
    fn handle(&self, input: &[u8]) -> Bytes {
        <T as RpcFn>::call(self, input)
    }
}

struct Noop;
impl RpcFn for Noop {
    const KEY: &'static str = "noop";
    fn call(&self, _input: &[u8]) -> Bytes {
        b"ok".to_vec()
    }
}

struct Echo;
impl RpcFn for Echo {
    const KEY: &'static str = "echo";
    fn call(&self, input: &[u8]) -> Bytes {
        input.to_vec()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Paradigm B: signature-bound shape  (axum's handler model, minus axum)
// ─────────────────────────────────────────────────────────────────────────────

/// `FromRequest`: how bytes become the handler's input — decided by a bound on
/// the signature, not by the handler body.
trait FromRequest: Sized {
    fn extract(input: &[u8]) -> Self;
}
/// `IntoResponse`: how the handler's output becomes bytes — same idea.
trait IntoResponse {
    fn respond(self) -> Bytes;
}

/// Identity extraction/response — the *cheapest possible* bound-side shape.
/// This isolates the pure "two extra trait calls per request" cost with no
/// real parsing, so we can see the mechanism overhead alone.
impl FromRequest for Bytes {
    fn extract(input: &[u8]) -> Self {
        input.to_vec()
    }
}
impl IntoResponse for Bytes {
    fn respond(self) -> Bytes {
        self
    }
}

/// The bound-side handler: `fn(In) -> Out`. Dispatch runs the pipeline
/// `In::extract -> f -> Out::respond`. The `In`/`Out` types are constrained by
/// the `FromRequest`/`IntoResponse` bounds — that is the "shape" living on the
/// signature.
struct BoundHandler<In, Out> {
    f: Box<dyn Fn(In) -> Out + Send + Sync>,
    _in: std::marker::PhantomData<In>,
    _out: std::marker::PhantomData<Out>,
}
impl<In: FromRequest + 'static, Out: IntoResponse + 'static> BoundHandler<In, Out> {
    fn new(f: impl Fn(In) -> Out + Send + Sync + 'static) -> Self {
        Self {
            f: Box::new(f),
            _in: std::marker::PhantomData,
            _out: std::marker::PhantomData,
        }
    }
}
impl<In: FromRequest + 'static, Out: IntoResponse + 'static> DynHandler for BoundHandler<In, Out> {
    fn handle(&self, input: &[u8]) -> Bytes {
        let req = In::extract(input);
        let resp = (self.f)(req);
        resp.respond()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A second bound-side shape that does *real* extraction: parse bytes into a
// structured `String` input and serialize the `String` output back. This is
// what axum's `Query<T>` / `Json<T>` actually do, and where the allocation
// cost of the signature-bound paradigm comes from.
#[derive(Clone)]
struct Text(String);
impl FromRequest for Text {
    fn extract(input: &[u8]) -> Self {
        // Models `Json<String>` / `Query`: allocate a String from bytes.
        Text(String::from_utf8_lossy(input).into_owned())
    }
}
impl IntoResponse for Text {
    fn respond(self) -> Bytes {
        // Models serializing the typed output back to bytes.
        self.0.into_bytes()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Table builders
// ─────────────────────────────────────────────────────────────────────────────

fn assoc_table() -> Table {
    let mut t = Table { routes: Vec::new() };
    t.insert(Noop::KEY, Box::new(Noop));
    t.insert(Echo::KEY, Box::new(Echo));
    t
}

fn bound_table_identity() -> Table {
    let mut t = Table { routes: Vec::new() };
    t.insert("noop", Box::new(BoundHandler::<Bytes, Bytes>::new(|_| b"ok".to_vec())));
    t.insert("echo", Box::new(BoundHandler::<Bytes, Bytes>::new(|b| b)));
    t
}

fn bound_table_real() -> Table {
    let mut t = Table { routes: Vec::new() };
    t.insert("noop", Box::new(BoundHandler::<Text, Text>::new(|_| Text("ok".into()))));
    t.insert("echo", Box::new(BoundHandler::<Text, Text>::new(|t| t)));
    t
}

// ─────────────────────────────────────────────────────────────────────────────
// Benchmarks
// ─────────────────────────────────────────────────────────────────────────────

fn bench_noop(c: &mut Criterion) {
    let assoc = assoc_table();
    let bound = bound_table_identity();
    let input = b"";

    let mut g = c.benchmark_group("shape/noop");
    g.bench_function("assoc_item", |b| b.iter(|| assoc.dispatch("noop", input)));
    g.bench_function("sig_bound/identity", |b| b.iter(|| bound.dispatch("noop", input)));
    g.finish();
}

fn bench_echo(c: &mut Criterion) {
    let assoc = assoc_table();
    let bound_id = bound_table_identity();
    let bound_real = bound_table_real();
    let input = b"hello paradigm";

    let mut g = c.benchmark_group("shape/echo");
    g.bench_function("assoc_item", |b| b.iter(|| assoc.dispatch("echo", input)));
    g.bench_function("sig_bound/identity", |b| b.iter(|| bound_id.dispatch("echo", input)));
    // The honest signature-bound cost: real extract + respond (allocates).
    g.bench_function("sig_bound/real_extract", |b| {
        b.iter(|| bound_real.dispatch("echo", input))
    });
    g.finish();
}

criterion_group!(paradigm, bench_noop, bench_echo);
criterion_main!(paradigm);
