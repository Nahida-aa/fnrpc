use proc_macro::TokenStream;
use quote::quote;
use syn::{
    FnArg, GenericArgument, ItemFn, LitStr, PathArguments, ReturnType, Type, TypePath, TypeReference,
    parse_macro_input,
    parse::Parse, parse::ParseStream,
};

struct RpcSubscribeAttr {
    method: Option<String>,
    path: Option<String>,
}

impl Parse for RpcSubscribeAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut method = None;
        let mut path = None;

        if !input.is_empty() {
            method = Some(input.parse::<LitStr>()?.value());
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
                path = Some(input.parse::<LitStr>()?.value());
            }
        }

        Ok(RpcSubscribeAttr { method, path })
    }
}

/// Extract the Output type from a stream return type like `impl Stream<Item = T>` or
/// `impl Stream<Item = Result<T, E>>`.  Returns `(Output_ts, is_result)`, where
/// `is_result` indicates whether the stream item is already `Result<T, E>`.
fn extract_stream_output(return_type: &ReturnType) -> (proc_macro2::TokenStream, bool) {
    let ty = match return_type {
        ReturnType::Type(_, ty) => ty.as_ref(),
        _ => panic!("subscribe function must have a stream return type"),
    };

    // Recursively find `Stream<Item = T>` inside impl Trait, TraitObject, or nested generics
    fn find_stream_item<'a>(ty: &'a Type) -> Option<&'a syn::Type> {
        match ty {
            Type::ImplTrait(impl_trait) => {
                for bound in &impl_trait.bounds {
                    if let syn::TypeParamBound::Trait(trait_bound) = bound {
                        if let Some(item) = item_from_trait_bound(trait_bound) {
                            return Some(item);
                        }
                    }
                }
                None
            }
            // Recurse into generic type arguments (e.g. Pin<Box<dyn ...>>)
            Type::Path(TypePath { path, .. }) => {
                for seg in &path.segments {
                    if let PathArguments::AngleBracketed(angled) = &seg.arguments {
                        for arg in &angled.args {
                            if let GenericArgument::Type(inner_ty) = arg {
                                if let Some(item) = find_stream_item(inner_ty) {
                                    return Some(item);
                                }
                            }
                        }
                    }
                }
                None
            }
            // dyn Stream<Item = T> + Send
            Type::TraitObject(trait_obj) => {
                for bound in &trait_obj.bounds {
                    if let syn::TypeParamBound::Trait(trait_bound) = bound {
                        if let Some(item) = item_from_trait_bound(trait_bound) {
                            return Some(item);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn item_from_trait_bound<'a>(trait_bound: &'a syn::TraitBound) -> Option<&'a syn::Type> {
        let last_seg = trait_bound.path.segments.last()?;
        if let PathArguments::AngleBracketed(angled) = &last_seg.arguments {
            for arg in &angled.args {
                if let GenericArgument::AssocType(assoc) = arg {
                    if assoc.ident == "Item" {
                        return Some(&assoc.ty);
                    }
                }
            }
        }
        None
    }

    let item_ty = find_stream_item(ty)
        .unwrap_or_else(|| panic!("could not find Stream<Item = T> in return type"));

    // Check if item is Result<Output, E>
    if let Type::Path(TypePath { path, .. }) = item_ty {
        if let Some(last_seg) = path.segments.last() {
            if last_seg.ident == "Result" {
                if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
                    if let Some(GenericArgument::Type(first)) = args.args.first() {
                        return (quote! { #first }, true);
                    }
                }
            }
        }
    }

    // Non-Result item: Output = T
    (quote! { #item_ty }, false)
}

pub(crate) fn rpc_subscribe_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let impl_fn_name = syn::Ident::new(&format!("{}_impl", fn_name), fn_name.span());
    let mut impl_fn = input_fn.clone();
    impl_fn.sig.ident = impl_fn_name.clone();

    // Parse attribute
    let sub_attr: RpcSubscribeAttr = parse_macro_input!(attr as RpcSubscribeAttr);
    let method_str = sub_attr.method.as_deref().unwrap_or("get");
    let path_str = sub_attr.path.unwrap_or_else(|| fn_name.to_string());
    let http_method = if method_str == "post" { "POST" } else { "GET" };

    // --- Analyse parameters (same as rpc_fn_impl) ---
    let params: Vec<&FnArg> = impl_fn.sig.inputs.iter().collect();

    let (has_ctx, ctx_ty) = if let Some(FnArg::Typed(pat)) = params.first() {
        if let Type::Reference(TypeReference { elem, .. }) = pat.ty.as_ref() {
            (true, quote! { #elem })
        } else {
            (false, quote! { () })
        }
    } else {
        (false, quote! { () })
    };

    let input_params: Vec<&FnArg> = if has_ctx {
        params.iter().copied().skip(1).collect()
    } else {
        params.iter().copied().collect()
    };

    // --- Build call expression (not async — subscribe exec is sync) ---
    let call = if input_params.is_empty() {
        if has_ctx {
            quote! { #impl_fn_name(ctx) }
        } else {
            quote! { #impl_fn_name() }
        }
    } else if input_params.len() == 1 {
        if has_ctx {
            quote! { #impl_fn_name(ctx, input) }
        } else {
            quote! { #impl_fn_name(input) }
        }
    } else {
        let destructure: Vec<_> = (0..input_params.len())
            .map(|i| {
                let idx = syn::Index::from(i);
                quote! { input.#idx }
            })
            .collect();
        if has_ctx {
            quote! { #impl_fn_name(ctx, #(#destructure),*) }
        } else {
            quote! { #impl_fn_name(#(#destructure),*) }
        }
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

    // --- Extract output type from stream item ---
    let (output_ty, is_result_item) = extract_stream_output(&input_fn.sig.output);

    // --- Build exec body ---
    let exec_body = if is_result_item {
        quote! {
            Box::pin({
                use ::futures::StreamExt;
                #call.map(|__item| __item.map_err(|__e: _| fnrpc::error::RpcErr::internal(__e.to_string())))
            })
        }
    } else {
        quote! {
            Box::pin({
                use ::futures::StreamExt;
                #call.map(|__item| Ok(__item))
            })
        }
    };

    let struct_name = fn_name.clone();
    let path_val = path_str.clone();

    let expanded = if has_ctx {
        quote! {
            #impl_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #struct_name;

            impl fnrpc::handler::RpcSubscribe<#ctx_ty> for #struct_name {
                type Input = #input_ty;
                type Output = #output_ty;
                const NAME: &'static str = stringify!(#fn_name);
                const METHOD: &'static str = #http_method;

                fn exec(
                    ctx: &#ctx_ty,
                    input: Self::Input,
                ) -> std::pin::Pin<Box<dyn ::futures::Stream<Item = Result<Self::Output, fnrpc::error::RpcErr>> + Send + 'static>> {
                    #exec_body
                }
            }

            impl fnrpc::handler::RoutedSubscribeHandler<#ctx_ty> for #struct_name {
                fn path() -> &'static str { #path_val }
                fn method() -> &'static str { #http_method }
            }
        }
    } else {
        quote! {
            #impl_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #struct_name;

            impl<T: Send + Sync + 'static> fnrpc::handler::RpcSubscribe<T> for #struct_name {
                type Input = #input_ty;
                type Output = #output_ty;
                const NAME: &'static str = stringify!(#fn_name);
                const METHOD: &'static str = #http_method;

                fn exec(
                    _ctx: &T,
                    input: Self::Input,
                ) -> std::pin::Pin<Box<dyn ::futures::Stream<Item = Result<Self::Output, fnrpc::error::RpcErr>> + Send + 'static>> {
                    #exec_body
                }
            }

            impl<T: Send + Sync + 'static> fnrpc::handler::RoutedSubscribeHandler<T> for #struct_name {
                fn path() -> &'static str { #path_val }
                fn method() -> &'static str { #http_method }
            }
        }
    };

    expanded.into()
}
