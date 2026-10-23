//! Parametric evaluation: document features become concrete kernel operations.
//!
//! Panels are built in their local frame: the profile lies in the XY plane and the extrusion runs
//! along `+Z`, so the panel occupies `z` from `0` (face `B`) to its thickness (face `A`). The frame
//! is anchored at the nominal minimum corner; the core, its machining and the bands are translated
//! by the resolved core origin so included banding fills the nominal edge.
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

use furnicraft_geom::{DVec2, DVec3, Frame};
use furnicraft_kernel::{BodyId, BooleanOp, KernelError, Profile, SolidKernel};
use furnicraft_model::{
    Cutout, CutoutDepth, CutoutShape, EdgeBanding, EdgeId, FaceSide, Machining, Panel, PanelGeometry,
};
use furnicraft_units::Length;
use thiserror::Error;

/// Extra millimetres added to a cutting tool so booleans do not depend on coincident faces.
const TOOL_OVERSHOOT_MM: f64 = 0.001;

/// An error raised while evaluating a parametric feature.
#[derive(Debug, Error)]
pub enum ParamError {
    /// The geometry kernel rejected or failed the operation.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// Banding refers to an edge outside the rectangular panel outline.
    #[error("edge banding on unsupported edge {edge}")]
    UnsupportedEdge {
        /// The rejected edge.
        edge: EdgeId,
    },
}

/// Builds the solid body of a rectangular panel in its local frame.
///
/// The panel frame is anchored at the nominal minimum corner. The core starts at
/// `(core_x, core_y)` and has its length along `+X`, its width along `+Y` and its thickness along
/// `+Z`. Panel-owned cutouts are core-local, so their tool bodies are translated by the same core
/// origin. A through cutout removes the full thickness, while a blind cutout removes material
/// from its entry face. The caller owns the returned handle and must eventually release it through
/// [`SolidKernel::clear`].
pub fn build_panel(kernel: &mut dyn SolidKernel, panel: &Panel, geometry: PanelGeometry) -> Result<BodyId, ParamError> {
    let core_x = geometry.core_x().millimetres_f64();
    let core_y = geometry.core_y().millimetres_f64();
    let length = geometry.core_length().millimetres_f64();
    let width = geometry.core_width().millimetres_f64();
    let thickness = geometry.thickness().millimetres_f64();
    let profile = Profile::rectangle(length, width)?;
    let frame = Frame { origin: DVec3::new(core_x, core_y, 0.0), ..Frame::WORLD };
    let mut body = kernel.extrude(&profile, &frame, thickness)?;
    for machining in panel.machining() {
        match machining {
            Machining::Cutout(cutout) => {
                let tool = build_cutout_tool(kernel, geometry, cutout)?;
                body = kernel.boolean(BooleanOp::Difference, body, tool)?;
            }
        }
    }
    Ok(body)
}

/// One evaluated edge-banding body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeBandPart {
    edge: EdgeId,
    body: BodyId,
}

impl EdgeBandPart {
    /// The covered edge.
    pub const fn edge(self) -> EdgeId {
        self.edge
    }

    /// The kernel body for this band.
    pub const fn body(self) -> BodyId {
        self.body
    }
}

/// The resolved cross-section of an edge band.
///
/// The engine resolves this from the session catalogue before evaluation; [`build_edge_bands`]
/// falls back to the profile stored with the banding when no catalogue is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeBandProfile {
    thickness: Length,
    width: Length,
}

impl EdgeBandProfile {
    /// Creates a profile from a band thickness and a band width.
    pub const fn new(thickness: Length, width: Length) -> Self {
        Self { thickness, width }
    }

    /// The profile stored with a banding record.
    pub fn from_band(band: &EdgeBanding) -> Self {
        Self { thickness: band.last_known_thickness(), width: band.last_known_width() }
    }

    /// The thickness across the panel edge.
    pub const fn thickness(self) -> Length {
        self.thickness
    }

    /// The width of the band face.
    pub const fn width(self) -> Length {
        self.width
    }
}

/// Builds one extruded body per banding in application order using the stored profiles.
///
/// Each band is a rectangle in the panel plane, extruded across the panel thickness. A band is
/// trimmed across the thickness when its profile is wider than the panel. At a right-angle corner,
/// a band extends by the thickness of an earlier band on the adjacent edge and covers the corner;
/// a band with no earlier neighbour ends flush with the core.
pub fn build_edge_bands(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
) -> Result<Vec<EdgeBandPart>, ParamError> {
    build_edge_bands_with(kernel, panel, geometry, EdgeBandProfile::from_band)
}

/// Builds edge bands with profiles resolved by the caller.
///
/// The engine passes catalogue-resolved profiles here; the parametric layer itself never reads a
/// catalogue. `profile` is called once per band, in application order.
pub fn build_edge_bands_with<F>(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
    band_profile: F,
) -> Result<Vec<EdgeBandPart>, ParamError>
where
    F: Fn(&EdgeBanding) -> EdgeBandProfile,
{
    let bands = panel.edge_banding();
    let mut parts = Vec::with_capacity(bands.len());
    for (index, band) in bands.iter().enumerate() {
        let rect = band_rect(geometry, bands, index, band, &band_profile)?;
        let profile = Profile::new(
            vec![
                DVec2::new(rect.x0, rect.y0),
                DVec2::new(rect.x1, rect.y0),
                DVec2::new(rect.x1, rect.y1),
                DVec2::new(rect.x0, rect.y1),
            ],
            Vec::new(),
        )?;
        let visible_width = band_profile(band).width().min(geometry.thickness());
        let panel_thickness = geometry.thickness().millimetres_f64();
        let visible_width = visible_width.millimetres_f64();
        let core_x = geometry.core_x().millimetres_f64();
        let core_y = geometry.core_y().millimetres_f64();
        let frame =
            Frame { origin: DVec3::new(core_x, core_y, (panel_thickness - visible_width) * 0.5), ..Frame::WORLD };
        let body = kernel.extrude(&profile, &frame, visible_width)?;
        parts.push(EdgeBandPart { edge: band.edge(), body });
    }
    Ok(parts)
}

struct BandRect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

fn band_rect<F>(
    geometry: PanelGeometry,
    bands: &[EdgeBanding],
    index: usize,
    band: &EdgeBanding,
    band_profile: &F,
) -> Result<BandRect, ParamError>
where
    F: Fn(&EdgeBanding) -> EdgeBandProfile,
{
    let length = geometry.core_length().millimetres_f64();
    let width = geometry.core_width().millimetres_f64();
    let thickness = band_profile(band).thickness().millimetres_f64();
    let (start_edge, end_edge) = match band.edge().get() {
        1 => (4, 2),
        2 => (1, 3),
        3 => (2, 4),
        4 => (3, 1),
        _ => return Err(ParamError::UnsupportedEdge { edge: band.edge() }),
    };
    let start = earlier_thickness(bands, index, start_edge, band_profile);
    let end = earlier_thickness(bands, index, end_edge, band_profile);
    let start = start.millimetres_f64();
    let end = end.millimetres_f64();
    let rect = match band.edge().get() {
        1 => BandRect { x0: -start, y0: -thickness, x1: length + end, y1: 0.0 },
        2 => BandRect { x0: length, y0: -start, x1: length + thickness, y1: width + end },
        3 => BandRect { x0: -end, y0: width, x1: length + start, y1: width + thickness },
        4 => BandRect { x0: -thickness, y0: -end, x1: 0.0, y1: width + start },
        _ => return Err(ParamError::UnsupportedEdge { edge: band.edge() }),
    };
    Ok(rect)
}

fn earlier_thickness<F>(bands: &[EdgeBanding], index: usize, edge: u32, band_profile: &F) -> Length
where
    F: Fn(&EdgeBanding) -> EdgeBandProfile,
{
    bands
        .get(..index)
        .and_then(|earlier| earlier.iter().find(|band| band.edge().get() == edge))
        .map_or(Length::ZERO, |band| band_profile(band).thickness())
}

fn build_cutout_tool(
    kernel: &mut dyn SolidKernel,
    geometry: PanelGeometry,
    cutout: &Cutout,
) -> Result<BodyId, ParamError> {
    let CutoutShape::Rectangle(rect) = cutout.shape();
    let thickness = geometry.thickness().millimetres_f64();
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
    let origin = DVec3::new(
        geometry.core_x().millimetres_f64() + rect.x().millimetres_f64(),
        geometry.core_y().millimetres_f64() + rect.y().millimetres_f64(),
        z,
    );
    let frame = Frame { origin, ..Frame::WORLD };
    Ok(kernel.extrude(&profile, &frame, height)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::DVec2;
    use furnicraft_kernel::{BooleanOp, Measure, TessOptions};
    use furnicraft_kernel_truck::TruckKernel;
    use furnicraft_model::{
        Assembly, EdgeBanding, EdgeId, Id, MaterialId, PanelNominalSize, PanelRect, Placement, Project,
    };
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

    fn panel_size() -> PanelNominalSize {
        PanelNominalSize::new(millimetres(700), millimetres(500)).expect("valid panel size")
    }

    fn sample_panel() -> Panel {
        Panel::new(Id::new(1).expect("id"), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("valid panel")
    }

    fn geometry(panel: &Panel) -> PanelGeometry {
        panel.geometry_with_fallbacks().expect("resolved panel geometry")
    }

    fn cutout_rect(x: i64, y: i64, width: i64, height: i64) -> PanelRect {
        PanelRect::new(millimetres(x), millimetres(y), millimetres(width), millimetres(height))
            .expect("valid cutout rectangle")
    }

    fn panel_with_cutout(shape: CutoutShape, depth: CutoutDepth, entry: FaceSide) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        project.add_cutout(panel_id, shape, depth, entry).expect("add cutout");
        project.panel(panel_id).expect("panel exists").clone()
    }

    fn edge_material() -> MaterialId {
        MaterialId::new(101).expect("material id")
    }

    fn panel_with_bands(bands: &[(u32, bool)], band_width: Length) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        for (edge, included) in bands {
            let band = EdgeBanding::new(
                EdgeId::new(*edge).expect("edge id"),
                edge_material(),
                *included,
                millimetres(2),
                band_width,
            )
            .expect("banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }
        project.panel(panel_id).expect("panel").clone()
    }

    fn band_volumes(panel: &Panel) -> Vec<f64> {
        band_volumes_with(panel, EdgeBandProfile::from_band)
    }

    fn band_volumes_with<F>(panel: &Panel, band_profile: F) -> Vec<f64>
    where
        F: Fn(&EdgeBanding) -> EdgeBandProfile,
    {
        let mut kernel = TruckKernel::new();
        let parts = build_edge_bands_with(&mut kernel, panel, geometry(panel), band_profile).expect("edge bands");
        let volumes = parts.iter().map(|part| kernel.measure(part.body()).expect("band measure").volume).collect();
        kernel.clear();
        volumes
    }

    #[test]
    fn build_panel_maps_dimensions_to_a_world_frame_extrusion() {
        let mut kernel = RecordingKernel::default();
        let panel = sample_panel();
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("panel body");

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
    fn build_panel_places_an_included_bottom_core_inside_the_nominal_frame() {
        let mut kernel = RecordingKernel::default();
        let panel = panel_with_bands(&[(1, true)], millimetres(22));
        let geometry = geometry(&panel);
        assert_eq!(geometry.core_y(), millimetres(2));

        build_panel(&mut kernel, &panel, geometry).expect("panel body");

        assert_eq!(kernel.frame, Some(Frame { origin: DVec3::new(0.0, 2.0, 0.0), ..Frame::WORLD }));
        let profile = kernel.profile.as_ref().expect("recorded profile");
        assert_eq!(
            profile.outer,
            vec![DVec2::new(0.0, 0.0), DVec2::new(700.0, 0.0), DVec2::new(700.0, 498.0), DVec2::new(0.0, 498.0),]
        );
    }

    #[test]
    fn included_bands_are_translated_with_the_core_origin() {
        let mut kernel = RecordingKernel::default();
        let bottom = panel_with_bands(&[(1, true)], millimetres(22));
        build_edge_bands_with(&mut kernel, &bottom, geometry(&bottom), EdgeBandProfile::from_band).expect("band");
        assert_eq!(kernel.frame, Some(Frame { origin: DVec3::new(0.0, 2.0, 0.0), ..Frame::WORLD }));
        let profile = kernel.profile.as_ref().expect("recorded band profile");
        assert_eq!(
            profile.outer,
            vec![DVec2::new(0.0, -2.0), DVec2::new(700.0, -2.0), DVec2::new(700.0, 0.0), DVec2::new(0.0, 0.0),]
        );

        let mut kernel = RecordingKernel::default();
        let left = panel_with_bands(&[(4, true)], millimetres(22));
        build_edge_bands_with(&mut kernel, &left, geometry(&left), EdgeBandProfile::from_band).expect("band");
        assert_eq!(kernel.frame, Some(Frame { origin: DVec3::new(2.0, 0.0, 0.0), ..Frame::WORLD }));
        let profile = kernel.profile.as_ref().expect("recorded band profile");
        assert_eq!(
            profile.outer,
            vec![DVec2::new(-2.0, 0.0), DVec2::new(0.0, 0.0), DVec2::new(0.0, 500.0), DVec2::new(-2.0, 500.0),]
        );
    }

    #[test]
    fn through_cutout_is_translated_with_the_included_core_origin() {
        let mut kernel = TruckKernel::new();
        let shape = CutoutShape::Rectangle(cutout_rect(1, 1, 10, 10));
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        for edge in [1, 4] {
            let band = EdgeBanding::new(
                EdgeId::new(edge).expect("edge id"),
                edge_material(),
                true,
                millimetres(2),
                millimetres(22),
            )
            .expect("banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        let panel = project.panel(panel_id).expect("panel").clone();
        let geometry = geometry(&panel);
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));

        let body = build_panel(&mut kernel, &panel, geometry).expect("panel body");
        let measure = kernel.measure(body).expect("panel measure");
        let expected = 698.0 * 498.0 * 18.0 - 10.0 * 10.0 * 18.0;
        assert!((measure.volume - expected).abs() < 1.0, "volume was {}, expected {expected}", measure.volume);
    }

    #[test]
    fn build_panel_propagates_kernel_errors() {
        let mut kernel = RecordingKernel { fail: true, ..RecordingKernel::default() };
        let panel = sample_panel();
        let error = build_panel(&mut kernel, &panel, geometry(&panel)).expect_err("kernel failure");
        assert!(matches!(error, ParamError::Kernel(KernelError::Failed(_))));
    }

    #[test]
    fn truck_kernel_builds_a_panel_with_the_expected_volume() {
        let mut kernel = TruckKernel::new();
        let panel = sample_panel();
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("panel body");

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
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("panel body");

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
            let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("panel body");

            let measure = kernel.measure(body).expect("panel measure");
            let expected = 6_300_000.0 - 100.0 * 50.0 * 5.0;
            assert!(
                (measure.volume - expected).abs() < 1.0,
                "volume for entry {entry:?} was {}, expected {expected}",
                measure.volume
            );
        }
    }
    #[test]
    fn edge_bands_are_trimmed_across_the_panel_thickness() {
        let wide = panel_with_bands(&[(1, false)], millimetres(22));
        let volumes = band_volumes(&wide);
        assert_eq!(volumes.len(), 1);
        assert!((volumes[0] - 700.0 * 2.0 * 18.0).abs() < 1.0, "wide band volume {}", volumes[0]);

        let narrow = panel_with_bands(&[(1, false)], millimetres(10));
        let volumes = band_volumes(&narrow);
        assert!((volumes[0] - 700.0 * 2.0 * 10.0).abs() < 1.0, "narrow band volume {}", volumes[0]);
    }

    #[test]
    fn edge_bands_use_caller_resolved_profiles() {
        let panel = panel_with_bands(&[(1, false), (2, false)], millimetres(22));
        let volumes = band_volumes_with(&panel, |_| EdgeBandProfile::new(millimetres(4), millimetres(10)));
        let expected = [700.0 * 4.0 * 10.0, (500.0 + 4.0) * 4.0 * 10.0];
        assert_eq!(volumes.len(), expected.len());
        for (index, (volume, expected)) in volumes.iter().zip(expected).enumerate() {
            assert!((volume - expected).abs() < 1.0, "band {index} volume {volume}, expected {expected}");
        }
    }

    #[test]
    fn edge_bands_extend_into_corners_in_application_order() {
        let panel = panel_with_bands(&[(1, false), (2, false), (3, false), (4, false)], millimetres(22));
        let volumes = band_volumes(&panel);
        assert_eq!(volumes.len(), 4);
        let expected = [
            700.0 * 2.0 * 18.0,
            (500.0 + 2.0) * 2.0 * 18.0,
            (700.0 + 2.0) * 2.0 * 18.0,
            (500.0 + 2.0 + 2.0) * 2.0 * 18.0,
        ];
        for (index, (volume, expected)) in volumes.iter().zip(expected).enumerate() {
            assert!((volume - expected).abs() < 1.0, "band {index} volume {volume}, expected {expected}");
        }
    }

    #[test]
    fn a_single_edge_band_does_not_extend_into_an_unbanded_corner() {
        let panel = panel_with_bands(&[(4, false)], millimetres(22));
        let volumes = band_volumes(&panel);
        assert!((volumes[0] - 500.0 * 2.0 * 18.0).abs() < 1.0, "left band volume {}", volumes[0]);
    }

    #[test]
    fn build_edge_bands_returns_nothing_without_banding() {
        let panel = sample_panel();
        let mut kernel = TruckKernel::new();
        let parts = build_edge_bands(&mut kernel, &panel, geometry(&panel)).expect("edge bands");
        assert!(parts.is_empty());
        assert_eq!(kernel.body_count(), 0);
    }
}
