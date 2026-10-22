//! The wgpu renderer.
//!
//! Owns pipelines, buffers, depth handling and id-buffer picking. Knows nothing about `egui` and never creates a window: the shell wires it into `egui_wgpu`.
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
