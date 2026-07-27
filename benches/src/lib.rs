// Benchmarks — no library code.
// `compare` is the dhat heap-allocation suite; it only builds with the
// `dhat-heap` feature (all its modules reference `dhat`).
#[cfg(feature = "dhat-heap")]
pub mod compare;
