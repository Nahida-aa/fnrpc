//! TypeScript code generation.
//!
//! Generates a `bindings.ts` file with:
//!
//! - All specta-exported type definitions
//! - A `Procedures` interface for full type-safe client creation
//! - A `__procedureMeta` runtime map with `{ kind, method }` per path
//!
//! Use [`generate_ts_client`] to get the string, or [`write_ts_client`]
//! to write directly to disk.

use std::path::Path;

use specta::Type;
use specta::datatype::{DataType, Primitive, Reference};

use crate::handler::TsTypeInfo;
use crate::router::RpcRouter;

/// Register a type into the shared specta Types registry.
/// Called during RpcRouterBuilder::route_fn at build time.
pub fn register_type<T: Type>(types: &mut specta::Types) {
    T::definition(types);
}

/// Resolve a [`DataType`] to a TypeScript type reference string.
/// Called after all types have been registered.
pub fn resolve_ts_ref(data_type: &DataType, types: &specta::Types) -> String {
    match data_type {
        DataType::Struct(_) | DataType::Enum(_) => types
            .clone()
            .into_sorted_iter()
            .next()
            .map(|ndt| ndt.name.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        DataType::Reference(Reference::Named(r)) => {
            if let Some(ndt) = types.get(r) {
                if ndt.ty.is_some() {
                    ndt.name.to_string()
                } else {
                    let exporter = specta_typescript::Typescript::default();
                    specta_typescript::primitives::inline(&exporter, types, data_type)
                        .unwrap_or_else(|_| "unknown".to_string())
                }
            } else {
                "unknown".to_string()
            }
        }
        DataType::Primitive(p)
            if matches!(
                p,
                Primitive::u64
                    | Primitive::i64
                    | Primitive::u128
                    | Primitive::i128
                    | Primitive::usize
                    | Primitive::isize
            ) =>
        {
            "bigint".to_string()
        }
        DataType::Primitive(Primitive::f64) => "number | null".to_string(),
        _ => {
            let exporter = specta_typescript::Typescript::default();
            specta_typescript::primitives::inline(&exporter, types, data_type)
                .unwrap_or_else(|_| "unknown".to_string())
        }
    }
}

/// Resolve a specta [`Type`] to a TypeScript type reference string,
/// registering it into the shared Types registry.
///
/// This is the convenience version for use in route_fn.
/// It creates a temporary Types for the type, registers it,
/// resolves the ts_ref, then merges into the shared types.
///
/// For production codegen, prefer calling [`register_type`] + [`resolve_ts_ref`]
/// separately with a shared Types to avoid cloning.
pub fn type_ts<T: Type>() -> TsTypeInfo {
    let mut types = specta::Types::default();
    let data_type = T::definition(&mut types);
    let ts_ref = resolve_ts_ref(&data_type, &types);
    TsTypeInfo { ts_ref }
}

/// No-op Format — doesn't modify types
struct NoFmt;
impl specta::Format for NoFmt {
    fn map_types(
        &self,
        types: &specta::Types,
    ) -> std::result::Result<std::borrow::Cow<'_, specta::Types>, specta::FormatError> {
        Ok(std::borrow::Cow::Owned(types.clone()))
    }
    fn map_type(
        &self,
        _types: &specta::Types,
        dt: &specta::datatype::DataType,
    ) -> std::result::Result<std::borrow::Cow<'_, specta::datatype::DataType>, specta::FormatError>
    {
        Ok(std::borrow::Cow::Owned(dt.clone()))
    }
}

/// Generate TypeScript type definitions and a `Procedures` interface.
///
/// The output includes:
///
/// - All specta-exported type definitions for input/output types.
/// - A `Procedures` type mapping each procedure name to its `{ kind, method, input, output, error }`.
/// - A `__procedureMeta` const map used at runtime by the TS client for dispatch.
pub fn generate_ts_client<Ctx: Send + Sync + 'static>(
    router: &RpcRouter<Ctx>,
    _rpc_url: &str,
) -> String {
    // Debug: print registered types
    eprintln!("Types count: {}", router.types.len());
    for ndt in router.types.into_sorted_iter() {
        eprintln!(
            "  type: {} (module={:?}, ty={})",
            ndt.name,
            ndt.module_path,
            ndt.ty.is_some()
        );
    }

    // Export all types from the shared registry at once.
    // NOTE: `export` fails (rather than silently skipping) when a type cannot
    // be represented in TypeScript — e.g. BigInt-style Rust integers (u64/i64/...)
    // are forbidden by specta-typescript to avoid precision loss in JS. We must
    // surface that error instead of swallowing it (the old `.unwrap_or_default()`
    // produced a `bindings.ts` missing type definitions while still emitting a
    // dangling `Procedures` interface).
    let exporter =
        specta_typescript::Typescript::default().header("// Auto-generated by fnrpc. DO NOT EDIT.");
    let export_str = match exporter.export(&router.types, NoFmt) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fnrpc: failed to generate TypeScript bindings:\n{e}");
            eprintln!(
                "fnrpc: hint — if this is a BigInt-style integer (u64/i64/usize/...), either \
                 use a smaller integer type, serialize it as a string, or override the field \
                 with `#[specta(type = specta_typescript::Number)]` to accept precision loss. \
                 See https://docs.rs/specta-typescript/latest/specta_typescript/struct.Error.html#bigint-forbidden"
            );
            panic!("fnrpc: TypeScript codegen aborted due to an unsupported type (see above)");
        }
    };

    let mut out = String::new();
    out.push_str("// Auto-generated by fnrpc. DO NOT EDIT.\n");
    out.push_str("// This file has been generated by Specta. Do not edit this file manually.\n");

    // Write all exported type definitions (skip duplicate header from exporter)
    if !export_str.is_empty() {
        for line in export_str.lines() {
            // Skip the header lines that exporter also emits
            if line.starts_with("// Auto-generated") || line.starts_with("// This file has been") {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }

    // Build the Procedures interface
    out.push_str("export type Procedures = {\n");
    for meta in router.procedures() {
        let kind = meta.kind;
        let method = meta.method;
        out.push_str(&format!(
            "  {}: {{ kind: \"{kind}\"; method: \"{method}\"; input: {}; output: {}; error: RpcErr }};\n",
            meta.key,
            meta.input.ts_ref,
            meta.output.ts_ref,
        ));
    }
    out.push_str("}\n");

    out.push_str("\nexport const __procedureMeta = {\n");
    for meta in router.procedures() {
        let kind = meta.kind;
        let method = meta.method;
        out.push_str(&format!(
            "  {}: {{ kind: \"{kind}\", method: \"{method}\" }},\n",
            meta.key,
        ));
    }
    out.push_str("} as const;\n");

    out
}

/// Generate and write a TypeScript client file to disk.
///
/// Shortcut for calling [`generate_ts_client`] and writing the result
/// with [`std::fs::write`].
///
/// # Errors
///
/// Returns `io::Error` if the file cannot be written.
pub fn write_ts_client<Ctx: Send + Sync + 'static>(
    router: &RpcRouter<Ctx>,
    rpc_url: &str,
    output_path: &Path,
) -> std::io::Result<()> {
    let content = generate_ts_client(router, rpc_url);
    std::fs::write(output_path, content)
}
