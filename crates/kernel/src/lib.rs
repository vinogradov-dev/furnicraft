//! The geometry kernel **port**.
//!
//! This crate defines what FurniCraft needs from solid geometry — profiles, opaque bodies,
//! extrusion, booleans, tessellation and measures — using only our own types. It deliberately
//! knows nothing about the third-party engine that implements it; `furnicraft-kernel-truck` is
//! the adapter. `model` and `param` depend on this port, never on the adapter.
//!
//! Coordinates in this crate are millimetres (`f64`). This is the geometry boundary: `param`
//! converts the document's `furnicraft_units::Length` values on the way in.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use furnicraft_geom::{DVec2, Frame, MeshData};
use thiserror::Error;

/// A geometry error that a caller can report to the user.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum KernelError {
    /// The input profile, frame, distance or body is not usable.
    #[error("invalid geometry input: {0}")]
    InvalidInput(String),
    /// This adapter does not implement the requested operation.
    #[error("geometry operation is not supported: {0}")]
    Unsupported(&'static str),
    /// The engine could not produce a valid result for a valid input.
    #[error("geometry operation failed: {0}")]
    Failed(String),
    /// The engine panicked; the panic was caught at the adapter boundary.
    #[error("geometry engine panicked: {0}")]
    Internal(String),
}

/// Result type used by the kernel port.
pub type Result<T> = std::result::Result<T, KernelError>;

/// A closed planar profile: one outer loop and zero or more holes.
///
/// The points form a closed polygon; the last point connects back to the first. Coordinates are
/// millimetres in the profile's local plane. The adapter normalises loop orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// Outer boundary loop, at least three points.
    pub outer: Vec<DVec2>,
    /// Hole loops, each at least three points.
    pub holes: Vec<Vec<DVec2>>,
}

impl Profile {
    /// Creates and validates a profile.
    pub fn new(outer: Vec<DVec2>, holes: Vec<Vec<DVec2>>) -> Result<Profile> {
        let profile = Profile { outer, holes };
        profile.validate()?;
        Ok(profile)
    }

    /// A rectangle from `(0, 0)` to `(width, height)`, counter-clockwise.
    pub fn rectangle(width: f64, height: f64) -> Result<Profile> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(KernelError::InvalidInput("rectangle needs a positive finite width and height".into()));
        }
        Profile::new(
            vec![DVec2::new(0.0, 0.0), DVec2::new(width, 0.0), DVec2::new(width, height), DVec2::new(0.0, height)],
            Vec::new(),
        )
    }

    /// The signed area of the outer loop (positive for counter-clockwise).
    pub fn signed_area(&self) -> f64 {
        signed_area(&self.outer)
    }

    /// The area between the outer loop and the holes, in square millimetres.
    pub fn area(&self) -> f64 {
        self.signed_area().abs() - self.holes.iter().map(|hole| signed_area(hole).abs()).sum::<f64>()
    }

    /// Checks that every loop is finite, has three or more points and bounds a non-zero area.
    ///
    /// Self-intersection, hole containment and overlapping holes are not checked here; the caller
    /// that constructs a feature is responsible for a sane profile until the kernel gains a
    /// dedicated validator.
    pub fn validate(&self) -> Result<()> {
        if self.outer.len() < 3 {
            return Err(KernelError::InvalidInput("a profile needs at least three outer points".into()));
        }
        for (index, point) in self.outer.iter().enumerate() {
            if !point.is_finite() {
                return Err(KernelError::InvalidInput(format!("outer point {index} is not finite")));
            }
        }
        if self.signed_area().abs() <= 1e-9 {
            return Err(KernelError::InvalidInput("the outer loop has zero area".into()));
        }
        for (hole_index, hole) in self.holes.iter().enumerate() {
            if hole.len() < 3 {
                return Err(KernelError::InvalidInput(format!("hole {hole_index} needs at least three points")));
            }
            if hole.iter().any(|point| !point.is_finite()) {
                return Err(KernelError::InvalidInput(format!("hole {hole_index} has a non-finite point")));
            }
            if signed_area(hole).abs() <= 1e-9 {
                return Err(KernelError::InvalidInput(format!("hole {hole_index} has zero area")));
            }
        }
        Ok(())
    }
}

/// Signed area of a closed polygon (shoelace formula).
pub fn signed_area(points: &[DVec2]) -> f64 {
    points.windows(2).map(|pair| pair[0].perp_dot(pair[1])).sum::<f64>() * 0.5
        + points.last().zip(points.first()).map_or(0.0, |(last, first)| last.perp_dot(*first) * 0.5)
}

/// A boolean operation between two bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    /// Keep the material of both bodies.
    Union,
    /// Remove the second body from the first.
    Difference,
    /// Keep only the shared material.
    Intersection,
}

/// An opaque handle to a body owned by a kernel adapter.
///
/// Handles are valid only for the adapter instance that created them, until [`SolidKernel::clear`],
/// or until a boolean consumes the body. Using a stale handle is an error, never a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(u32);

impl BodyId {
    /// Creates a handle from its raw index. Adapters use this to mint handles.
    pub const fn from_raw(index: u32) -> BodyId {
        BodyId(index)
    }

    /// The raw index, for adapter-side storage.
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Tessellation quality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TessOptions {
    /// Maximum chordal deviation in millimetres.
    pub deflection: f64,
}

impl Default for TessOptions {
    fn default() -> Self {
        TessOptions { deflection: 0.05 }
    }
}

impl TessOptions {
    /// Creates options with the given deflection.
    pub fn new(deflection: f64) -> Result<TessOptions> {
        if !deflection.is_finite() || deflection <= 0.0 {
            return Err(KernelError::InvalidInput("tessellation deflection must be positive and finite".into()));
        }
        Ok(TessOptions { deflection })
    }
}

/// Derived measurements of a body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measure {
    /// Enclosed volume in cubic millimetres.
    pub volume: f64,
}

/// What the rest of FurniCraft needs from a solid modelling engine.
///
/// The port is intentionally small and shape-driven: it exposes primitive operations, not
/// furniture features. `param` composes panels, pockets, chamfers and cutter paths from these;
/// linear extrusions are here now, curved sweeps come with panel machining.
pub trait SolidKernel {
    /// Extrudes `profile` from `frame` towards `frame.z` by `height` millimetres.
    fn extrude(&mut self, profile: &Profile, frame: &Frame, height: f64) -> Result<BodyId>;

    /// Applies a boolean operation to two bodies and returns the result.
    ///
    /// Both operands are consumed: their handles become invalid. The result is a new handle.
    fn boolean(&mut self, op: BooleanOp, target: BodyId, tool: BodyId) -> Result<BodyId>;

    /// Tessellates a body into an indexed triangle mesh.
    fn tessellate(&self, body: BodyId, options: &TessOptions) -> Result<MeshData>;

    /// Measures a body.
    fn measure(&self, body: BodyId) -> Result<Measure>;

    /// Drops every body. Called between evaluation passes.
    fn clear(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_has_expected_area() {
        let profile = Profile::rectangle(700.0, 500.0).expect("rectangle");
        assert!((profile.area() - 350_000.0).abs() < 1e-6);
        assert!(profile.signed_area() > 0.0);
    }

    #[test]
    fn profile_with_hole_subtracts_area() {
        let hole = vec![DVec2::new(10.0, 10.0), DVec2::new(20.0, 10.0), DVec2::new(20.0, 20.0), DVec2::new(10.0, 20.0)];
        let profile =
            Profile::new(Profile::rectangle(100.0, 100.0).expect("outer").outer, vec![hole]).expect("profile");
        assert!((profile.area() - (10_000.0 - 100.0)).abs() < 1e-6);
    }

    #[test]
    fn degenerate_profiles_are_rejected() {
        assert!(Profile::new(Vec::new(), Vec::new()).is_err());
        assert!(Profile::new(vec![DVec2::ZERO, DVec2::X], Vec::new()).is_err());
        assert!(Profile::rectangle(0.0, 10.0).is_err());
        assert!(Profile::rectangle(f64::NAN, 10.0).is_err());
    }

    #[test]
    fn tessellation_options_are_validated() {
        assert!(TessOptions::new(0.05).is_ok());
        assert!(TessOptions::new(0.0).is_err());
        assert!(TessOptions::new(f64::INFINITY).is_err());
    }
}
