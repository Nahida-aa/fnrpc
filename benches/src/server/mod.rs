// fnrpc variants — always available (path deps)
pub(crate) mod fnrpc_web;
pub(crate) mod fnrpc_xitca;
pub(crate) mod fnrpc_axum;

// plain frameworks — gated
#[cfg(feature = "xitca-web-plain")]
pub(crate) mod xitca_web;
#[cfg(feature = "axum-plain")]
pub(crate) mod axum;
#[cfg(feature = "actix-web")]
pub(crate) mod actix;
#[cfg(feature = "ntex")]
pub(crate) mod ntex;
