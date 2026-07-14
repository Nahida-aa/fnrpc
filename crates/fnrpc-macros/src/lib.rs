use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, FnArg, GenericArgument, ItemFn, PathArguments, ReturnType, Type,
    TypePath, TypeReference,
};

struct RegistryInput {
    ctx_ty: syn::Type,
    query_fns: Vec<syn::Path>,
    mutation_fns: Vec<syn::Path>,
    subscription_fns: Vec<syn::Path>,
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
        let mut mutation_fns = Vec::new();
        let mut subscription_fns = Vec::new();

        while !content.is_empty() {
            let section: syn::Ident = content.parse()?;
            content.parse::<syn::Token![:]>()?;
            let items;
            syn::bracketed!(items in content);
            let target = if section == "queries" {
                &mut query_fns
            } else if section == "mutations" {
                &mut mutation_fns
            } else if section == "subscriptions" {
                &mut subscription_fns
            } else {
                return Err(syn::Error::new(
                    section.span(),
                    "expected `queries`, `mutations`, or `subscriptions`",
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
            mutation_fns,
            subscription_fns,
        })
    }
}

#[proc_macro]
pub fn fnrpc_registry(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as RegistryInput);
    let ctx_ty = &input.ctx_ty;

    let query_structs: Vec<syn::Path> = input
        .query_fns
        .iter()
        .map(fn_rpc_struct_path)
        .collect();
    let mutation_structs: Vec<syn::Path> = input
        .mutation_fns
        .iter()
        .map(fn_rpc_struct_path)
        .collect();
    let subscription_structs: Vec<syn::Path> = input
        .subscription_fns
        .iter()
        .map(fn_rpc_struct_path)
        .collect();

    quote! {
        pub fn build_fn_rpc() -> fnrpc::router::RpcRouter<#ctx_ty> {
            fnrpc::router::RpcRouter::new()
                #(.route(#query_structs))*
                #(.route(#mutation_structs))*
                #(.subscribe(#subscription_structs))*
        }
    }
    .into()
}

fn rpc_fn_impl(kind: &str, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;

    // --- Analyse parameters: infer Ctx from first param type ---
    // If first param is `&T`, it's the context (Ctx = T); otherwise no context (Ctx = ())
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
    let (output_ty, is_result_return) = match &input_fn.sig.output {
        ReturnType::Type(_, ty) => {
            if let Type::Path(type_path) = ty.as_ref() {
                let last_seg = type_path.path.segments.last().unwrap();
                if last_seg.ident == "Result" {
                    if let PathArguments::AngleBracketed(args) = &last_seg.arguments {
                        match args.args.first().unwrap() {
                            GenericArgument::Type(t) => (quote! { #t }, true),
                            _ => panic!("expected type in Result<T, E>"),
                        }
                    } else {
                        panic!("expected Result<T, E>");
                    }
                } else {
                    (quote! { #ty }, false)
                }
            } else {
                (quote! { #ty }, false)
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
        quote! {
            match #call {
                Ok(val) => Ok(val),
                Err(e) => Err(fnrpc::error::RpcErr(e.to_string())),
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
            .iter().copied()
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

#[proc_macro_attribute]
pub fn rpc_query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    rpc_fn_impl("query", item)
}

#[proc_macro_attribute]
pub fn rpc_mutation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    rpc_fn_impl("mutation", item)
}

#[proc_macro_attribute]
pub fn rpc_subscription(_attr: TokenStream, item: TokenStream) -> TokenStream {
    rpc_subscription_impl(item)
}

/// Extract the Output type from a stream return type like `impl Stream<Item = T>` or
/// `impl Stream<Item = Result<T, E>>`.  Returns `(Output_ts, is_result)`, where
/// `is_result` indicates whether the stream item is already `Result<T, E>`.
fn extract_stream_output(return_type: &ReturnType) -> (proc_macro2::TokenStream, bool) {
    let ty = match return_type {
        ReturnType::Type(_, ty) => ty.as_ref(),
        _ => panic!("subscription function must have a stream return type"),
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

fn rpc_subscription_impl(item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;

    // --- Analyse parameters (same as rpc_fn_impl) ---
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

    let input_params: Vec<&FnArg> = if has_ctx {
        params.iter().copied().skip(1).collect()
    } else {
        params.iter().copied().collect()
    };

    // --- Build call expression (not async — subscription exec is sync) ---
    let call = if input_params.is_empty() {
        if has_ctx {
            quote! { #fn_name(ctx) }
        } else {
            quote! { #fn_name() }
        }
    } else if input_params.len() == 1 {
        if has_ctx {
            quote! { #fn_name(ctx, input) }
        } else {
            quote! { #fn_name(input) }
        }
    } else {
        let destructure: Vec<_> = (0..input_params.len())
            .map(|i| {
                let idx = syn::Index::from(i);
                quote! { input.#idx }
            })
            .collect();
        if has_ctx {
            quote! { #fn_name(ctx, #(#destructure),*) }
        } else {
            quote! { #fn_name(#(#destructure),*) }
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
            .iter().copied()
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
        // User's stream already yields Result<Output, E>, map error to RpcErr
        quote! {
            Box::pin({
                use ::futures::StreamExt;
                #call.map(|__item| __item.map_err(|__e: _| fnrpc::error::RpcErr(__e.to_string())))
            })
        }
    } else {
        // User's stream yields Output directly, wrap in Ok
        quote! {
            Box::pin({
                use ::futures::StreamExt;
                #call.map(|__item| Ok(__item))
            })
        }
    };

    let struct_name = syn::Ident::new(&format!("{}__FnRpc", fn_name), fn_name.span());

    let expanded = if has_ctx {
        quote! {
            #input_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #struct_name;

            impl fnrpc::handler::RpcSubscription<#ctx_ty> for #struct_name {
                type Input = #input_ty;
                type Output = #output_ty;
                const NAME: &'static str = stringify!(#fn_name);

                fn exec(
                    ctx: &#ctx_ty,
                    input: Self::Input,
                ) -> std::pin::Pin<Box<dyn ::futures::Stream<Item = Result<Self::Output, fnrpc::error::RpcErr>> + Send + '_>> {
                    #exec_body
                }
            }
        }
    } else {
        quote! {
            #input_fn

            #[allow(non_camel_case_types, dead_code)]
            #fn_vis struct #struct_name;

            impl<T: Send + Sync + 'static> fnrpc::handler::RpcSubscription<T> for #struct_name {
                type Input = #input_ty;
                type Output = #output_ty;
                const NAME: &'static str = stringify!(#fn_name);

                fn exec(
                    _ctx: &T,
                    input: Self::Input,
                ) -> std::pin::Pin<Box<dyn ::futures::Stream<Item = Result<Self::Output, fnrpc::error::RpcErr>> + Send + '_>> {
                    #exec_body
                }
            }
        }
    };

    expanded.into()
}
