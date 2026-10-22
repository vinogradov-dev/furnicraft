//! Renderer-agnostic scene description: what a frame contains.
//!
//! Drawables, transforms, colours, selection state, camera and lights as plain data, so the scene can be tested without a GPU and rendered by any backend.
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
