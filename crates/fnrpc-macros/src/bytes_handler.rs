use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr, Type, TypeReference, parse_macro_input, parse::Parse, parse::ParseStream};

struct BytesAttr {
    key: Option<String>,
}

impl Parse for BytesAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key = if !input.is_empty() {
            Some(input.parse::<LitStr>()?.value())
        } else {
            None
        };
        Ok(BytesAttr { key })
    }
}

pub(crate) fn rpc_bytes_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let impl_fn_name = syn::Ident::new(&format!("{}_impl", fn_name), fn_name.span());
    let mut impl_fn = input_fn.clone();
    impl_fn.sig.ident = impl_fn_name.clone();

    let attr: BytesAttr = parse_macro_input!(attr as BytesAttr);
    let key = attr.key.unwrap_or_else(|| fn_name.to_string());

    // Analyse parameters: detect Ctx from first &T param (not &[u8])
    let params: Vec<&FnArg> = impl_fn.sig.inputs.iter().collect();

    let (has_ctx, ctx_ty) = if let Some(FnArg::Typed(pat)) = params.first() {
        if let Type::Reference(TypeReference { elem, .. }) = pat.ty.as_ref() {
            // Check it's not &[u8] (which is the input parameter)
            match elem.as_ref() {
                Type::Slice(_) => (false, quote! { () }),
                _ => (true, quote! { #elem }),
            }
        } else {
            (false, quote! { () })
        }
    } else {
        (false, quote! { () })
    };

    // Build the call expression
    let call = if has_ctx {
        quote! { #impl_fn_name(ctx, input) }
    } else {
        quote! { #impl_fn_name(input) }
    };

    // For async functions, the call returns a future that needs to be awaited
    let call_expr = if input_fn.sig.asyncness.is_some() {
        quote! { #call.await }
    } else {
        call
    };

    let expanded = if has_ctx {
        quote! {
            #impl_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #fn_name;

            impl fnrpc::handler::RawRpcFn<#ctx_ty> for #fn_name {
                const KEY: &'static str = #key;
                fn exec<'a>(ctx: &'a #ctx_ty, input: &'a [u8]) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<Vec<u8>, fnrpc::error::RpcErr>> + Send + 'a>> {
                    Box::pin(async move { Ok(#call_expr) })
                }
            }
        }
    } else {
        quote! {
            #impl_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #fn_name;

            impl fnrpc::handler::RawRpcFn<()> for #fn_name {
                const KEY: &'static str = #key;
                fn exec<'a>(_ctx: &'a (), input: &'a [u8]) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<Vec<u8>, fnrpc::error::RpcErr>> + Send + 'a>> {
                    Box::pin(async move { Ok(#call_expr) })
                }
            }
        }
    };

    expanded.into()
}
