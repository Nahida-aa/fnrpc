use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use serde_json::Value;

use crate::error::RpcErr;
use crate::handler::{ErasedHandler, ErasedSubscriptionHandler};
use crate::middleware::{FnLayer, FnService};

pub struct RpcRouter<Ctx> {
    inner: Arc<RpcRouterInner<Ctx>>,
}

struct RpcRouterInner<Ctx> {
    handlers: HashMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>,
    subscribes: HashMap<&'static str, Arc<dyn ErasedSubscriptionHandler<Ctx>>>,
    layers: Vec<Box<dyn FnLayer<Ctx>>>,
}

impl<Ctx> Clone for RpcRouter<Ctx> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<Ctx> RpcRouter<Ctx>
where
    Ctx: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RpcRouterInner {
                handlers: HashMap::new(),
                subscribes: HashMap::new(),
                layers: Vec::new(),
            }),
        }
    }

    pub fn route<H: ErasedHandler<Ctx> + 'static>(self, handler: H) -> Self {
        let name = handler.name();
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.handlers.insert(name, Arc::new(handler));
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Register a subscribe handler.
    pub fn subscribe<H: ErasedSubscriptionHandler<Ctx> + 'static>(self, handler: H) -> Self {
        let name = handler.name();
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.subscribes.insert(name, Arc::new(handler));
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Attach a middleware layer.
    ///
    /// Similar to Hono's `.use()`, TanStack Start's `.middleware()`,
    /// or Tower's `.layer()`. The last call becomes the outermost layer.
    pub fn layer<L: FnLayer<Ctx> + 'static>(self, layer: L) -> Self {
        let mut inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| unreachable!("consumed self => sole owner"));
        inner.layers.push(Box::new(layer));
        Self {
            inner: Arc::new(inner),
        }
    }

    pub async fn dispatch(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let mut svc: Box<dyn FnService<Ctx>> = Box::new(RouterService {
            handlers: self.inner.handlers.clone(),
        });
        for layer in self.inner.layers.iter() {
            svc = layer.layer(svc);
        }
        svc.call(ctx, path, input).await
    }

    /// Retrieve a subscribe handler by path (owned `Arc` for `'static` usage).
    pub fn get_sub_handler(&self, path: &str) -> Option<Arc<dyn ErasedSubscriptionHandler<Ctx>>> {
        self.inner.subscribes.get(path).cloned()
    }

    /// Dispatch a subscribe by path, returning a stream of values.
    pub fn dispatch_subscribe<'a>(
        &'a self,
        ctx: &'a Ctx,
        path: &str,
        input: Value,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Value, RpcErr>> + Send + 'a>>, RpcErr> {
        let handler = self
            .inner
            .subscribes
            .get(path)
            .ok_or_else(|| RpcErr(format!("unknown subscribe path: {path}")))?;
        Ok(handler.call(ctx, input))
    }

    /// Generate TypeScript type definitions and a Procedures interface.
    pub fn generate_ts_client(&self, _rpc_url: &str) -> String {
        use specta::datatype::DataType;

        // No-op Format — doesn't modify types
        struct NoFmt;
        impl specta::Format for NoFmt {
            fn map_types(
                &self,
                types: &specta::Types,
            ) -> std::result::Result<std::borrow::Cow<'_, specta::Types>, specta::FormatError>
            {
                Ok(std::borrow::Cow::Owned(types.clone()))
            }
            fn map_type(
                &self,
                _types: &specta::Types,
                dt: &DataType,
            ) -> std::result::Result<std::borrow::Cow<'_, DataType>, specta::FormatError>
            {
                Ok(std::borrow::Cow::Owned(dt.clone()))
            }
        }

        // Collect types into a shared type registry
        let mut types = specta::Types::default();
        for (_, handler) in &self.inner.handlers {
            handler.populate_types(&mut types, &mut vec![]);
        }
        for (_, sub) in &self.inner.subscribes {
            sub.populate_types(&mut types, &mut vec![]);
        }

        // Apply semantic remapping for TS types.
        //
        // - `enable_lossless_bigints()`: u64/i64/u128/i128/usize/isize → `bigint`
        //   (default: error — specta forbids bigint to avoid silent precision loss)
        // - If your runtime supports JSON `number` with precision loss, use
        //   `#[specta(type = Number)]` per-field instead.
        //
        // f64 → `number | null` is the DEFAULT exporter behaviour (not from this config),
        // because JSON cannot represent NaN/Infinity/-Infinity — serde_json serialises
        // them as `null`.  If your transport layer preserves them losslessly, add:
        //   `.enable_lossless_floats()`
        // to flatten it back to plain `number`.
        let semantic =
            specta_typescript::semantic::Configuration::default().enable_lossless_bigints();
        let types = semantic.apply_types(&types);

        // Export all types via the exporter
        let exporter = specta_typescript::Typescript::default()
            .header("// Auto-generated by fnrpc. DO NOT EDIT.");
        let mut out = exporter.export(&types, NoFmt).unwrap_or_default();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }

        // Build the Procedures interface
        out.push_str("export type Procedures = {\n");
        for (_, handler) in &self.inner.handlers {
            let i = handler.input_ts();
            let o = handler.output_ts();
            let kind = handler.kind();
            out.push_str(&format!(
                "  {}: {{ kind: \"{kind}\"; input: {}; output: {}; error: unknown }};\n",
                handler.name(),
                i.ts_ref,
                o.ts_ref,
            ));
        }
        for (_, sub) in &self.inner.subscribes {
            let i = sub.input_ts();
            let o = sub.output_ts();
            out.push_str(&format!(
                "  {}: {{ kind: \"subscribe\"; input: {}; output: {}; error: unknown }};\n",
                sub.name(),
                i.ts_ref,
                o.ts_ref,
            ));
        }
        out.push_str("}\n");

        out
    }

    /// Generate and write a TypeScript client file to disk.
    pub fn write_ts_client(&self, rpc_url: &str, output_path: &Path) -> std::io::Result<()> {
        let content = self.generate_ts_client(rpc_url);
        std::fs::write(output_path, content)
    }
}

/// Inner service that dispatches to handlers directly.
struct RouterService<Ctx> {
    handlers: HashMap<&'static str, Arc<dyn ErasedHandler<Ctx>>>,
}

#[async_trait::async_trait]
impl<Ctx: Send + Sync + 'static> FnService<Ctx> for RouterService<Ctx> {
    async fn call(&self, ctx: &Ctx, path: &str, input: Value) -> Result<Value, RpcErr> {
        let handler = self
            .handlers
            .get(path)
            .ok_or_else(|| RpcErr(format!("unknown path: {path}")))?;
        handler.call(ctx, input).await
    }
}
