mod query;
mod subscribe;
mod registry;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn rpc_query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    query::rpc_fn_impl("query", item)
}

#[proc_macro_attribute]
pub fn rpc_mutate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    query::rpc_fn_impl("mutate", item)
}

#[proc_macro_attribute]
pub fn rpc_subscribe(_attr: TokenStream, item: TokenStream) -> TokenStream {
    subscribe::rpc_subscribe_impl(item)
}

#[proc_macro]
pub fn fnrpc_registry(input: TokenStream) -> TokenStream {
    registry::fnrpc_registry_impl(input)
}
