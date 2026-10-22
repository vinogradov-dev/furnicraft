//! Session state, the command registry and undo/redo.
//!
//! Every user-visible change to the document is a command with a stable id and a `Result`. This crate must not depend on `wgpu` or `egui`.
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
