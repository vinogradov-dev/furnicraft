//! Triangle meshes derived from the document model.
//!
//! Turns a panel, its machining and its edge banding into indexed geometry with normals and texture coordinates. Owns no GPU state.
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
