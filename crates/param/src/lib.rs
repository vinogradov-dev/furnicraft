//! Parametric evaluation: the assembly's dimensions become concrete parts and placements.
//!
//! Reports validation errors as typed results; never panics on a broken parametric graph.
//!
//! Scaffold only: the crate is intentionally empty until its milestone starts.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
