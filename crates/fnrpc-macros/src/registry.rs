use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

struct RegistryInput {
    ctx_ty: syn::Type,
    query_fns: Vec<syn::Path>,
    mutate_fns: Vec<syn::Path>,
    subscribe_fns: Vec<syn::Path>,
}

/// Given `handlers::log::watch_task_log`, return `handlers::log::watch_task_log__FnRpc`.
fn fn_rpc_struct_path(path: &syn::Path) -> syn::Path {
    let mut new_path = path.clone();
    if let Some(last) = new_path.segments.last_mut() {
        let name = format!("{}__FnRpc", last.ident);
        last.ident = syn::Ident::new(&name, last.ident.span());
    }
    new_path
}

impl syn::parse::Parse for RegistryInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let kw: syn::Ident = input.parse()?;
        if kw != "Router" {
            return Err(syn::Error::new(kw.span(), "expected `Router`"));
        }
        input.parse::<syn::Token![<]>()?;
        let ctx_ty: syn::Type = input.parse()?;
        input.parse::<syn::Token![>]>()?;

        let content;
        syn::braced!(content in input);

        let mut query_fns = Vec::new();
        let mut mutate_fns = Vec::new();
        let mut subscribe_fns = Vec::new();

        while !content.is_empty() {
            let section: syn::Ident = content.parse()?;
            content.parse::<syn::Token![:]>()?;
            let items;
            syn::bracketed!(items in content);
            let target = if section == "queries" {
                &mut query_fns
            } else if section == "mutates" {
                &mut mutate_fns
            } else if section == "subscribes" {
                &mut subscribe_fns
            } else {
                return Err(syn::Error::new(
                    section.span(),
                    "expected `queries`, `mutates`, or `subscribes`",
                ));
            };
            while !items.is_empty() {
                let path: syn::Path = items.parse()?;
                target.push(path);
                if items.is_empty() {
                    break;
                }
                let _: syn::Token![,] = items.parse()?;
            }
            if content.is_empty() {
                break;
            }
            let _: syn::Token![,] = content.parse()?;
        }

        Ok(RegistryInput {
            ctx_ty,
            query_fns,
            mutate_fns,
            subscribe_fns,
        })
    }
}

pub(crate) fn fnrpc_registry_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as RegistryInput);
    let ctx_ty = &input.ctx_ty;

    let query_structs: Vec<syn::Path> = input.query_fns.iter().map(fn_rpc_struct_path).collect();
    let mutate_structs: Vec<syn::Path> = input.mutate_fns.iter().map(fn_rpc_struct_path).collect();
    let subscribe_structs: Vec<syn::Path> =
        input.subscribe_fns.iter().map(fn_rpc_struct_path).collect();

    quote! {
        pub fn build_fn_rpc() -> fnrpc::router::RpcRouter<#ctx_ty> {
            fnrpc::router::RpcRouter::new()
                #(.route(#query_structs))*
                #(.route(#mutate_structs))*
                #(.subscribe(#subscribe_structs))*
        }
    }
    .into()
}
