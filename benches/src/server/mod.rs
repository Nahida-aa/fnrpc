// fnrpc variants — removed, will be replaced by compare/ benchmarks
// after fnrpc-xitca is fully developed.

// plain frameworks — gated
#[cfg(feature = "xitca-web-plain")]
pub(crate) mod xitca_web;
#[cfg(feature = "axum-plain")]
pub(crate) mod axum;
#[cfg(feature = "actix-web")]
pub(crate) mod actix;
#[cfg(feature = "ntex")]
pub(crate) mod ntex;
