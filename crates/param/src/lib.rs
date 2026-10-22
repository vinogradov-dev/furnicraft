//! Parametric evaluation: document features become concrete kernel operations.
//!
//! Panels are built in their local frame: the profile lies in the XY plane and the extrusion runs
//! along `+Z`, so the panel occupies `z` from `0` (face `B`) to its thickness (face `A`).
//! Panel-owned cutouts are subtracted as rectangular tool bodies, with blind cuts starting at the
//! named entry face. Placement into an assembly is a later concern and never happens here.
//!
//! Reports validation errors as typed results; never panics on a broken parametric graph.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use furnicraft_geom::{DVec3, Frame};
use furnicraft_kernel::{BodyId, BooleanOp, KernelError, Profile, SolidKernel};
use furnicraft_model::{Cutout, CutoutDepth, CutoutShape, FaceSide, Machining, Panel, PanelSize};
use thiserror::Error;

/// Extra millimetres added to a cutting tool so booleans do not depend on coincident faces.
const TOOL_OVERSHOOT_MM: f64 = 0.001;

/// An error raised while evaluating a parametric feature.
#[derive(Debug, Error)]
pub enum ParamError {
    /// The geometry kernel rejected or failed the operation.
    #[error(transparent)]
    Kernel(#[from] KernelError),
}

/// Builds the solid body of a rectangular panel in its local frame.
///
/// The body starts at the panel zero, has its length along `+X`, its width along `+Y` and its
/// thickness along `+Z`. Panel-owned cutouts are removed with boolean subtraction: a through
/// cutout removes the full thickness, while a blind cutout removes material from its entry face.
/// The caller owns the returned handle and must eventually release it through
/// [`SolidKernel::clear`].
pub fn build_panel(kernel: &mut dyn SolidKernel, panel: &Panel) -> Result<BodyId, ParamError> {
    let size = panel.size();
    let length = size.length().millimetres_f64();
    let width = size.width().millimetres_f64();
    let thickness = size.thickness().millimetres_f64();
    let profile = Profile::rectangle(length, width)?;
    let mut body = kernel.extrude(&profile, &Frame::WORLD, thickness)?;
    for machining in panel.machining() {
        match machining {
            Machining::Cutout(cutout) => {
                let tool = build_cutout_tool(kernel, size, cutout)?;
                body = kernel.boolean(BooleanOp::Difference, body, tool)?;
            }
        }
    }
    Ok(body)
}

fn build_cutout_tool(kernel: &mut dyn SolidKernel, size: PanelSize, cutout: &Cutout) -> Result<BodyId, ParamError> {
    let CutoutShape::Rectangle(rect) = cutout.shape();
    let thickness = size.thickness().millimetres_f64();
    let (z, height) = match cutout.depth() {
        CutoutDepth::Through => (-TOOL_OVERSHOOT_MM, thickness + 2.0 * TOOL_OVERSHOOT_MM),
        CutoutDepth::Blind { depth } => {
            let depth = depth.millimetres_f64();
            match cutout.entry() {
                FaceSide::A => (thickness - depth, depth + TOOL_OVERSHOOT_MM),
                FaceSide::B => (-TOOL_OVERSHOOT_MM, depth + TOOL_OVERSHOOT_MM),
            }
        }
    };
    let profile = Profile::rectangle(rect.width().millimetres_f64(), rect.height().millimetres_f64())?;
    let frame = Frame { origin: DVec3::new(rect.x().millimetres_f64(), rect.y().millimetres_f64(), z), ..Frame::WORLD };
    Ok(kernel.extrude(&profile, &frame, height)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::DVec2;
    use furnicraft_kernel::{BooleanOp, Measure, TessOptions};
    use furnicraft_kernel_truck::TruckKernel;
    use furnicraft_model::{Assembly, Id, PanelRect, Placement, Project};
    use furnicraft_units::Length;

    use super::*;

    #[derive(Default)]
    struct RecordingKernel {
        profile: Option<Profile>,
        frame: Option<Frame>,
        height: Option<f64>,
        fail: bool,
    }

    impl SolidKernel for RecordingKernel {
        fn extrude(&mut self, profile: &Profile, frame: &Frame, height: f64) -> furnicraft_kernel::Result<BodyId> {
            self.profile = Some(profile.clone());
            self.frame = Some(*frame);
            self.height = Some(height);
            if self.fail {
                return Err(KernelError::Failed("recording kernel was asked to fail".into()));
            }
            Ok(BodyId::from_raw(7))
        }

        fn boolean(&mut self, _op: BooleanOp, _target: BodyId, _tool: BodyId) -> furnicraft_kernel::Result<BodyId> {
            Err(KernelError::Unsupported("recording kernel does not implement booleans"))
        }

        fn tessellate(
            &self,
            _body: BodyId,
            _options: &TessOptions,
        ) -> furnicraft_kernel::Result<furnicraft_geom::MeshData> {
            Err(KernelError::Unsupported("recording kernel does not implement tessellation"))
        }

        fn measure(&self, _body: BodyId) -> furnicraft_kernel::Result<Measure> {
            Err(KernelError::Unsupported("recording kernel does not implement measurement"))
        }

        fn clear(&mut self) {
            self.profile = None;
            self.frame = None;
            self.height = None;
        }
    }

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    fn panel_size() -> PanelSize {
        PanelSize::new(millimetres(700), millimetres(500), millimetres(18)).expect("valid panel size")
    }

    fn sample_panel() -> Panel {
        Panel::new(Id::new(1).expect("id"), "Panel", panel_size(), Placement::IDENTITY)
    }

    fn cutout_rect(x: i64, y: i64, width: i64, height: i64) -> PanelRect {
        PanelRect::new(millimetres(x), millimetres(y), millimetres(width), millimetres(height))
            .expect("valid cutout rectangle")
    }

    fn panel_with_cutout(shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id =
            project.add_panel(project.root().id(), "Panel", panel_size(), Placement::IDENTITY).expect("add panel");
        project.add_cutout(panel_id, shape, depth, entry).expect("add cutout");
        project.panel(panel_id).expect("panel exists").clone()
    }

    #[test]
    fn build_panel_maps_dimensions_to_a_world_frame_extrusion() {
        let mut kernel = RecordingKernel::default();
        let body = build_panel(&mut kernel, &sample_panel()).expect("panel body");

        assert_eq!(body, BodyId::from_raw(7));
        let profile = kernel.profile.as_ref().expect("recorded profile");
        assert_eq!(
            profile.outer,
            vec![DVec2::new(0.0, 0.0), DVec2::new(700.0, 0.0), DVec2::new(700.0, 500.0), DVec2::new(0.0, 500.0),]
        );
        assert!(profile.holes.is_empty());
        assert_eq!(kernel.frame, Some(Frame::WORLD));
        assert_eq!(kernel.height, Some(18.0));
    }

    #[test]
    fn build_panel_propagates_kernel_errors() {
        let mut kernel = RecordingKernel { fail: true, ..RecordingKernel::default() };
        let error = build_panel(&mut kernel, &sample_panel()).expect_err("kernel failure");
        assert!(matches!(error, ParamError::Kernel(KernelError::Failed(_))));
    }

    #[test]
    fn truck_kernel_builds_a_panel_with_the_expected_volume() {
        let mut kernel = TruckKernel::new();
        let body = build_panel(&mut kernel, &sample_panel()).expect("panel body");

        let measure = kernel.measure(body).expect("panel measure");
        assert!((measure.volume - 6_300_000.0).abs() < 1.0, "volume was {}", measure.volume);
        assert_eq!(kernel.body_count(), 1);

        kernel.clear();
        assert_eq!(kernel.body_count(), 0);
    }

    #[test]
    fn through_cutout_removes_its_volume() {
        let mut kernel = TruckKernel::new();
        let shape = CutoutShape::Rectangle(cutout_rect(100, 100, 100, 50));
        let panel = panel_with_cutout(shape, CutoutDepth::Through, FaceSide::A);
        let body = build_panel(&mut kernel, &panel).expect("panel body");

        let measure = kernel.measure(body).expect("panel measure");
        let expected = 6_300_000.0 - 100.0 * 50.0 * 18.0;
        assert!((measure.volume - expected).abs() < 1.0, "volume was {}, expected {expected}", measure.volume);
    }

    #[test]
    fn blind_cutout_removes_its_volume_from_either_face() {
        for entry in [FaceSide::A, FaceSide::B] {
            let mut kernel = TruckKernel::new();
            let shape = CutoutShape::Rectangle(cutout_rect(100, 100, 100, 50));
            let depth = CutoutDepth::Blind { depth: millimetres(5) };
            let panel = panel_with_cutout(shape, depth, entry);
            let body = build_panel(&mut kernel, &panel).expect("panel body");

            let measure = kernel.measure(body).expect("panel measure");
            let expected = 6_300_000.0 - 100.0 * 50.0 * 5.0;
            assert!(
                (measure.volume - expected).abs() < 1.0,
                "volume for entry {entry:?} was {}, expected {expected}",
                measure.volume
            );
        }
    }
}
