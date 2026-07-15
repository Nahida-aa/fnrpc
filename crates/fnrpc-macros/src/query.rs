use proc_macro::TokenStream;
use quote::quote;
use syn::{
    FnArg, GenericArgument, ItemFn, PathArguments, ReturnType, Type, TypeReference,
    parse_macro_input,
};

/// Check if `E` in `Result<T, E>` is `RpcErr`.
fn is_rpc_err_type(arg: &syn::GenericArgument) -> bool {
    if let syn::GenericArgument::Type(syn::Type::Path(type_path)) = arg {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "RpcErr";
        }
    }
    false
}

pub(crate) fn rpc_fn_impl(kind: &str, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;

    // --- Analyse parameters: infer Ctx from first param type ---
    let params: Vec<&FnArg> = input_fn.sig.inputs.iter().collect();

    let (has_ctx, ctx_ty) = if let Some(FnArg::Typed(pat)) = params.first() {
        if let Type::Reference(TypeReference { elem, .. }) = pat.ty.as_ref() {
            (true, quote! { #elem })
        } else {
            (false, quote! { () })
        }
    } else {
        (false, quote! { () })
    };

    // Collect non-context parameters
    let input_params: Vec<&FnArg> = if has_ctx {
        params.iter().copied().skip(1).collect()
    } else {
        params.iter().copied().collect()
    };

    // --- Extract output type (auto-wrap non-Result in Ok) ---
    let (output_ty, is_result_return, is_rpc_err) = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => {
            if let Type::Path(type_path) = ty.as_ref() {
                let last_seg = type_path.path.segments.last().unwrap();
                if last_seg.ident == "Result" {
                    if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
                        match args.args.first().unwrap() {
                            GenericArgument::Type(t) => {
                                let is_rpc_err = args.args.len() > 1 && is_rpc_err_type(&args.args[1]);
                                (quote! { #t }, true, is_rpc_err)
                            }
                            _ => panic!("expected type in Result<T, E>"),
                        }
                    } else {
                        panic!("expected Result<T, E>");
                    }
                } else {
                    (quote! { #ty }, false, false)
                }
            } else {
                (quote! { #ty }, false, false)
            }
        }
        ReturnType::Default => panic!("function must have a return type"),
    };

    // --- Build the call expression to the original function ---
    let call = if input_params.is_empty() {
        if has_ctx {
            quote! { #fn_name(ctx).await }
        } else {
            quote! { #fn_name().await }
        }
    } else if input_params.len() == 1 {
        if has_ctx {
            quote! { #fn_name(ctx, input).await }
        } else {
            quote! { #fn_name(input).await }
        }
    } else {
        let destructure: Vec<_> = (0..input_params.len())
            .map(|i| {
                let idx = syn::Index::from(i);
                quote! { input.#idx }
            })
            .collect();
        if has_ctx {
            quote! { #fn_name(ctx, #(#destructure),*).await }
        } else {
            quote! { #fn_name(#(#destructure),*).await }
        }
    };

    let exec_body = if is_result_return {
        if is_rpc_err {
            quote! { #call }
        } else {
            quote! {
                match #call {
                    Ok(val) => Ok(val),
                    Err(e) => Err(fnrpc::error::RpcErr::internal(e.to_string())),
                }
            }
        }
    } else {
        quote! { Ok(#call) }
    };

    // --- Extract input type (tuple-ize multiple params) ---
    let input_ty: proc_macro2::TokenStream = if input_params.is_empty() {
        quote! { () }
    } else if input_params.len() == 1 {
        match input_params[0] {
            FnArg::Typed(pat_type) => {
                let ty = &pat_type.ty;
                quote! { #ty }
            }
            _ => panic!("parameter must be typed"),
        }
    } else {
        let types: Vec<_> = input_params
            .iter()
            .copied()
            .map(|arg| match arg {
                FnArg::Typed(pat_type) => &pat_type.ty,
                _ => panic!("parameter must be typed"),
            })
            .collect();
        quote! { (#(#types,)*) }
    };

    let struct_name = syn::Ident::new(&format!("{}__FnRpc", fn_name), fn_name.span());

    let expanded = if has_ctx {
        quote! {
            #input_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #struct_name;

            #[async_trait::async_trait]
            impl fnrpc::handler::RpcFn<#ctx_ty> for #struct_name {
                type Input = #input_ty;
                type Output = #output_ty;
                const NAME: &'static str = stringify!(#fn_name);
                const KIND: &'static str = #kind;

                async fn exec(ctx: &#ctx_ty, input: Self::Input) -> Result<Self::Output, fnrpc::error::RpcErr> {
                    #exec_body
                }
            }
        }
    } else {
        quote! {
            #input_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #struct_name;

            #[async_trait::async_trait]
            impl<T: Send + Sync + 'static> fnrpc::handler::RpcFn<T> for #struct_name {
                type Input = #input_ty;
                type Output = #output_ty;
                const NAME: &'static str = stringify!(#fn_name);
                const KIND: &'static str = #kind;

                async fn exec(_ctx: &T, input: Self::Input) -> Result<Self::Output, fnrpc::error::RpcErr> {
                    #exec_body
                }
            }
        }
    };

    expanded.into()
}
