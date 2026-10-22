//! The egui shell: panels, widgets and input wiring.
//!
//! Thin by design: actions are dispatched to the engine as commands, and the UI never mutates the document directly.
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
