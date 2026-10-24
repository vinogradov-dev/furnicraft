//! Parametric evaluation: document features become concrete kernel operations.
//!
//! Panels are built in their local frame: the profile lies in the XY plane and the extrusion runs
//! along `+Z`, so the panel occupies `z` from `0` (face `B`) to its thickness (face `A`). The frame
//! is anchored at the nominal minimum corner. The core and bands are placed relative to the
//! resolved core origin so included banding fills the nominal edge; cutouts stay in the nominal
//! panel frame and do not move with the core.
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

use furnicraft_geom::{Contour, ContourError, DVec2, DVec3, Frame, Line, Segment};
use furnicraft_kernel::{BodyId, BooleanOp, KernelError, Profile, SolidKernel};
use furnicraft_model::{
    BandingSpec, ContourPath, Cutout, CutoutDepth, CutoutShape, EdgeBanding, EdgeId, FaceSide, Machining, Panel,
    PanelContourError, PanelGeometry, VertexId,
};
use furnicraft_units::Length;
use thiserror::Error;

/// Extra millimetres added to a cutting tool so booleans do not depend on coincident faces.
const TOOL_OVERSHOOT_MM: f64 = 0.001;
/// Chord tolerance used when an analytic contour becomes a kernel polyline.
const CONTOUR_SAMPLE_TOLERANCE_MM: f64 = 0.05;

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
    /// The nominal contour cannot be built.
    #[error(transparent)]
    Contour(#[from] PanelContourError),
    /// The contour cannot be sampled for the polygon-only kernel profile.
    #[error(transparent)]
    ContourSampling(#[from] ContourError),
    /// Shaped panels do not support machining until the contour boolean layer lands.
    #[error("shaped panels do not support machining yet")]
    UnsupportedContourMachining,
}

/// Builds the solid body of a rectangular panel in its local frame.
///
/// The panel frame is anchored at the nominal minimum corner. The core starts at
/// `(core_x, core_y)` and has its length along `+X`, its width along `+Y` and its thickness along
/// `+Z`. Panel-owned cutouts are nominal panel-local, so their tool bodies are placed directly at
/// the stored coordinates without a core-origin translation. A through cutout removes the full
/// thickness, while a blind cutout removes material from its entry face. The caller owns the
/// returned handle and must eventually release it through [`SolidKernel::clear`].
pub fn build_panel(kernel: &mut dyn SolidKernel, panel: &Panel, geometry: PanelGeometry) -> Result<BodyId, ParamError> {
    if panel.has_contour_features() {
        if !panel.machining().is_empty() {
            return Err(ParamError::UnsupportedContourMachining);
        }
        return build_shaped_body_with(kernel, panel, geometry, &EdgeBandProfile::from_spec);
    }

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

/// What a band body covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandSource {
    /// The band follows an edge path; the edge is the first edge in that path.
    Edge(EdgeId),
    /// The band follows a lone chamfer.
    Corner(VertexId),
}

/// One evaluated banding body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandPart {
    source: BandSource,
    body: BodyId,
}

impl BandPart {
    /// The edge or chamfer this band covers.
    pub const fn source(self) -> BandSource {
        self.source
    }

    /// The kernel body for this band.
    pub const fn body(self) -> BodyId {
        self.body
    }
}

/// The evaluated body and bands of one panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelParts {
    body: BodyId,
    bands: Vec<BandPart>,
}

impl PanelParts {
    /// The panel body with included bands trimmed out.
    pub const fn body(&self) -> BodyId {
        self.body
    }

    /// The band bodies, in application order.
    pub fn bands(&self) -> &[BandPart] {
        &self.bands
    }
}

/// The resolved cross-section of an edge band.
///
/// The engine resolves this from the session catalogue before evaluation; [`build_panel_parts_with`]
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

    /// The fallback profile stored in a shared banding spec.
    pub fn from_spec(spec: &BandingSpec) -> Self {
        Self { thickness: spec.last_known_thickness(), width: spec.last_known_width() }
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

/// Builds one extruded body per rectangular banding in application order.
///
/// Each band is a rectangle in the panel plane, extruded across the panel thickness. A band is
/// trimmed across the thickness when its profile is wider than the panel. At a right-angle corner,
/// a band extends by the thickness of an earlier band on the adjacent edge and covers the corner;
/// a band with no earlier neighbour ends flush with the core.
fn build_rect_bands_with<F>(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
    band_profile: F,
) -> Result<Vec<BandPart>, ParamError>
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
        parts.push(BandPart { source: BandSource::Edge(band.edge()), body });
    }
    Ok(parts)
}

/// Builds the panel body and its bands with catalogue-resolved profiles.
///
/// Rectangular panels keep the v1 band rectangles and corner extension order. Shaped panels build a
/// core outline from per-path offsets and one strip per tangent-continuous path, so the body and the
/// visible band share the same resolved thickness without a boolean.
pub fn build_panel_parts_with<F>(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
    band_profile: F,
) -> Result<PanelParts, ParamError>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    if panel.has_contour_features() {
        if !panel.machining().is_empty() {
            return Err(ParamError::UnsupportedContourMachining);
        }
        let body = build_shaped_body_with(kernel, panel, geometry, &band_profile)?;
        let bands = build_shaped_bands(kernel, panel, geometry, &band_profile)?;
        return Ok(PanelParts { body, bands });
    }

    let body = build_panel(kernel, panel, geometry)?;
    let bands = build_rect_bands_with(kernel, panel, geometry, |band| band_profile(&band.spec()))?;
    Ok(PanelParts { body, bands })
}

fn build_shaped_body_with<F>(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
    band_profile: &F,
) -> Result<BodyId, ParamError>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let core = shaped_core_contour(panel, band_profile)?;
    let points = core.sample(CONTOUR_SAMPLE_TOLERANCE_MM)?;
    let profile = Profile::new(points, Vec::new())?;
    let thickness = geometry.thickness().millimetres_f64();
    Ok(kernel.extrude(&profile, &Frame::WORLD, thickness)?)
}

/// Builds the core outline of a shaped panel.
///
/// Each path is offset inward when its band is included; paths meet at sharp joints with a straight
/// butt connector. Unbanded and non-included paths stay on the nominal outline.
fn shaped_core_contour<F>(panel: &Panel, band_profile: &F) -> Result<Contour, ParamError>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let paths = panel.contour_paths()?;
    if paths.len() == 1 && paths[0].is_closed() {
        let path = &paths[0];
        let distance = panel
            .path_banding(path)
            .filter(|spec| spec.included_in_size())
            .map_or(0.0, |spec| band_profile(&spec).thickness().millimetres_f64());
        return Ok(if distance > 0.0 { path.contour().offset(distance)? } else { path.contour().clone() });
    }

    let mut path_segments = Vec::with_capacity(paths.len());
    let mut ends = Vec::with_capacity(paths.len());
    for path in &paths {
        let distance = panel
            .path_banding(path)
            .filter(|spec| spec.included_in_size())
            .map_or(0.0, |spec| band_profile(&spec).thickness().millimetres_f64());
        let offset = if distance > 0.0 { path.contour().offset(distance)? } else { path.contour().clone() };
        let start = offset.start().ok_or(ContourError::Empty)?;
        let end = offset.end().ok_or(ContourError::Empty)?;
        path_segments.push(offset.segments().to_vec());
        ends.push((start, end));
    }
    if ends.is_empty() {
        return Err(ContourError::Empty.into());
    }
    let mut segments = Vec::new();
    for index in 0..ends.len() {
        let Some(path) = path_segments.get(index) else {
            return Err(ContourError::Empty.into());
        };
        segments.extend_from_slice(path);
        let end = ends.get(index).map(|(_, end)| *end).ok_or(ContourError::Empty)?;
        let next = ends.get((index + 1) % ends.len()).map(|(start, _)| *start).ok_or(ContourError::Empty)?;
        if end.distance(next) > 1.0e-6 {
            segments.push(Segment::Line(Line::new(end, next)?));
        }
    }
    Ok(Contour::new(segments, true)?)
}

fn build_shaped_bands<F>(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
    band_profile: &F,
) -> Result<Vec<BandPart>, ParamError>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let panel_thickness = geometry.thickness();
    let mut parts = Vec::new();
    for path in panel.contour_paths()? {
        let Some(spec) = panel.path_banding(&path) else {
            continue;
        };
        let Some(source) = path_band_source(panel, &path) else {
            continue;
        };
        let profile = band_profile(&spec);
        let strip = band_strip_profile(&path, spec, profile)?;
        let visible_width = profile.width().min(panel_thickness).millimetres_f64();
        let panel_thickness_mm = panel_thickness.millimetres_f64();
        let z = (panel_thickness_mm - visible_width) * 0.5;
        let frame = Frame { origin: DVec3::new(0.0, 0.0, z), ..Frame::WORLD };
        let body = kernel.extrude(&strip, &frame, visible_width)?;
        parts.push(BandPart { source, body });
    }
    Ok(parts)
}

fn path_band_source(panel: &Panel, path: &ContourPath) -> Option<BandSource> {
    if let Some(corner) = path.lone_corner() {
        return panel.corner_banding_at(corner).map(|_| BandSource::Corner(corner));
    }
    path.edge_ids().next().map(BandSource::Edge)
}

fn band_strip_profile(path: &ContourPath, spec: BandingSpec, profile: EdgeBandProfile) -> Result<Profile, ParamError> {
    let distance = profile.thickness().millimetres_f64();
    let (outer, inner) = if spec.included_in_size() {
        (path.contour().clone(), path.contour().offset(distance)?)
    } else {
        (path.contour().offset(-distance)?, path.contour().clone())
    };
    let outer_points = outer.sample(CONTOUR_SAMPLE_TOLERANCE_MM)?;
    let inner_points = inner.sample(CONTOUR_SAMPLE_TOLERANCE_MM)?;
    if path.is_closed() {
        return Ok(Profile::new(outer_points, vec![inner_points])?);
    }
    let mut points = outer_points;
    points.extend(inner_points.iter().rev().copied());
    Ok(Profile::new(points, Vec::new())?)
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
    let origin = DVec3::new(rect.x().millimetres_f64(), rect.y().millimetres_f64(), z);
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
        Assembly, CornerBanding, CornerFeature, CornerFeatureKind, EdgeBanding, EdgeBow, EdgeId, Id, MaterialId,
        PanelNominalSize, PanelRect, Placement, Project, RectCorner, RectEdge,
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

    fn panel_with_corner(kind: CornerFeatureKind) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        let feature = CornerFeature::new(RectCorner::BottomRight.id(), kind).expect("corner feature");
        project.set_corner_feature(panel_id, feature).expect("set corner");
        project.panel(panel_id).expect("panel").clone()
    }

    fn panel_with_radius_and_band(included: bool) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(RectCorner::BottomRight.id(), CornerFeatureKind::Radius { radius: millimetres(25) })
                    .expect("radius"),
            )
            .expect("set corner");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), edge_material(), included, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("set band");
        project.panel(panel_id).expect("panel").clone()
    }

    fn panel_with_chamfer_band(included: bool) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        let corner = RectCorner::BottomRight.id();
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(20), outgoing: millimetres(10) },
                )
                .expect("chamfer"),
            )
            .expect("set corner");
        project
            .set_corner_banding(
                panel_id,
                CornerBanding::new(corner, edge_material(), included, millimetres(2), millimetres(22))
                    .expect("corner band"),
            )
            .expect("set corner band");
        project.panel(panel_id).expect("panel").clone()
    }

    fn panel_with_mixed_bands() -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), edge_material(), true, millimetres(2), millimetres(22))
                    .expect("bottom band"),
            )
            .expect("band bottom");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Right.id(), edge_material(), false, millimetres(2), millimetres(22))
                    .expect("right band"),
            )
            .expect("band right");
        let corner = RectCorner::BottomLeft.id();
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(30), outgoing: millimetres(30) },
                )
                .expect("chamfer"),
            )
            .expect("set chamfer");
        project
            .set_corner_banding(
                panel_id,
                CornerBanding::new(corner, edge_material(), true, millimetres(2), millimetres(22))
                    .expect("corner band"),
            )
            .expect("band chamfer");
        project.panel(panel_id).expect("panel").clone()
    }

    fn panel_with_bow(sagitta: Length) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        let bow = EdgeBow::new(RectEdge::Bottom.id(), sagitta).expect("bow");
        project.set_edge_bow(panel_id, bow).expect("set bow");
        project.panel(panel_id).expect("panel").clone()
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

    fn panel_with_cutouts(cutouts: &[(CutoutShape, CutoutDepth)]) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        for (shape, depth) in cutouts {
            project.add_cutout(panel_id, *shape, *depth, FaceSide::A).expect("add cutout");
        }
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

    fn panel_with_bands_and_cutout(bands: &[(u32, bool)], shape: CutoutShape) -> Panel {
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
                millimetres(22),
            )
            .expect("banding");
            project.set_edge_banding(panel_id, band).expect("set banding");
        }
        project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        project.panel(panel_id).expect("panel").clone()
    }

    fn band_volumes(panel: &Panel) -> Vec<f64> {
        band_volumes_with(panel, EdgeBandProfile::from_spec)
    }

    fn band_volumes_with<F>(panel: &Panel, band_profile: F) -> Vec<f64>
    where
        F: Fn(&BandingSpec) -> EdgeBandProfile,
    {
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, panel, geometry(panel), band_profile).expect("panel parts");
        let volumes =
            parts.bands().iter().map(|part| kernel.measure(part.body()).expect("band measure").volume).collect();
        kernel.clear();
        volumes
    }

    #[test]
    fn included_and_non_included_bands_meet_at_a_sharp_joint() {
        let panel = panel_with_mixed_bands();
        let nominal_volume = panel.nominal_contour().expect("contour").signed_area().expect("area") * 18.0;
        let right_band_volume = 2.0 * 500.0 * 18.0;
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("mixed panel parts");
        let body_volume = kernel.measure(parts.body()).expect("body measure").volume;
        let band_volume: f64 =
            parts.bands().iter().map(|part| kernel.measure(part.body()).expect("band measure").volume).sum();
        let expected = nominal_volume + right_band_volume;
        assert!(
            (body_volume + band_volume - expected).abs() < expected * 1.0e-3,
            "body {body_volume} + bands {band_volume} vs expected {expected}"
        );
        kernel.clear();
    }

    #[test]
    fn build_panel_samples_the_nominal_contour_for_a_shaped_panel() {
        let mut kernel = RecordingKernel::default();
        let panel = panel_with_corner(CornerFeatureKind::Radius { radius: millimetres(25) });
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("shaped panel body");

        assert_eq!(body, BodyId::from_raw(7));
        let profile = kernel.profile.as_ref().expect("recorded profile");
        assert!(profile.outer.len() > 4);
        assert!(profile.outer.iter().all(|point| *point != DVec2::new(700.0, 0.0)));
        assert!(profile.holes.is_empty());
        assert_eq!(kernel.frame, Some(Frame::WORLD));
        assert_eq!(kernel.height, Some(18.0));
    }

    #[test]
    fn truck_kernel_builds_a_chamfered_panel_with_the_cut_corner_removed() {
        let mut kernel = TruckKernel::new();
        let panel =
            panel_with_corner(CornerFeatureKind::Chamfer { incoming: millimetres(20), outgoing: millimetres(10) });
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("shaped panel body");

        let measure = kernel.measure(body).expect("panel measure");
        let expected = (700.0 * 500.0 - 0.5 * 20.0 * 10.0) * 18.0;
        assert!((measure.volume - expected).abs() < 1.0, "volume was {}, expected {expected}", measure.volume);
    }

    #[test]
    fn a_rounded_path_produces_one_band_strip() {
        let panel = panel_with_radius_and_band(false);
        assert_eq!(panel.edge_banding().len(), 2, "the path copies its spec to both edges");
        let mut kernel = RecordingKernel::default();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("rounded panel parts");
        assert_eq!(parts.bands().len(), 1);
        assert_eq!(parts.bands()[0].source(), BandSource::Edge(RectEdge::Bottom.id()));
    }

    #[test]
    fn included_band_on_a_rounded_path_fills_the_trimmed_core() {
        let panel = panel_with_radius_and_band(true);
        let nominal_area = panel.nominal_contour().expect("contour").signed_area().expect("area");
        let nominal_volume = nominal_area * 18.0;
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("rounded panel parts");
        let body_volume = kernel.measure(parts.body()).expect("body measure").volume;
        let band_volume: f64 =
            parts.bands().iter().map(|part| kernel.measure(part.body()).expect("band measure").volume).sum();
        assert!(body_volume < nominal_volume, "included band did not trim the core");
        assert!(
            (body_volume + band_volume - nominal_volume).abs() < nominal_volume * 1.0e-3,
            "body {body_volume} + band {band_volume} vs nominal {nominal_volume}"
        );
        kernel.clear();
    }

    #[test]
    fn a_non_included_band_on_a_rounded_path_stays_outside_the_body() {
        let panel = panel_with_radius_and_band(false);
        let nominal_area = panel.nominal_contour().expect("contour").signed_area().expect("area");
        let nominal_volume = nominal_area * 18.0;
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("rounded panel parts");
        let body_volume = kernel.measure(parts.body()).expect("body measure").volume;
        let band_volume: f64 =
            parts.bands().iter().map(|part| kernel.measure(part.body()).expect("band measure").volume).sum();
        assert!((body_volume - nominal_volume).abs() < nominal_volume * 1.0e-3);
        assert!(band_volume > 0.0);
        kernel.clear();
    }

    #[test]
    fn a_chamfer_corner_band_is_its_own_strip() {
        let panel = panel_with_chamfer_band(true);
        let nominal_area = panel.nominal_contour().expect("contour").signed_area().expect("area");
        let nominal_volume = nominal_area * 18.0;
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("chamfer panel parts");
        assert_eq!(parts.bands().len(), 1);
        assert_eq!(parts.bands()[0].source(), BandSource::Corner(RectCorner::BottomRight.id()));
        let body_volume = kernel.measure(parts.body()).expect("body measure").volume;
        let band_volume = kernel.measure(parts.bands()[0].body()).expect("band measure").volume;
        assert!(body_volume < nominal_volume);
        assert!(
            (body_volume + band_volume - nominal_volume).abs() < nominal_volume * 1.0e-3,
            "body {body_volume} + band {band_volume} vs nominal {nominal_volume}"
        );
        kernel.clear();
    }

    #[test]
    fn outward_bow_extends_the_extruded_profile_outside_the_nominal_frame() {
        let mut kernel = RecordingKernel::default();
        let panel = panel_with_bow(millimetres(10));
        build_panel(&mut kernel, &panel, geometry(&panel)).expect("bowed panel body");
        let profile = kernel.profile.as_ref().expect("recorded profile");
        let min_y = profile.outer.iter().map(|point| point.y).fold(f64::INFINITY, f64::min);
        assert!((min_y + 10.0).abs() < 0.1, "min y was {min_y}");
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
        build_panel_parts_with(&mut kernel, &bottom, geometry(&bottom), EdgeBandProfile::from_spec).expect("band");
        assert_eq!(kernel.frame, Some(Frame { origin: DVec3::new(0.0, 2.0, 0.0), ..Frame::WORLD }));
        let profile = kernel.profile.as_ref().expect("recorded band profile");
        assert_eq!(
            profile.outer,
            vec![DVec2::new(0.0, -2.0), DVec2::new(700.0, -2.0), DVec2::new(700.0, 0.0), DVec2::new(0.0, 0.0),]
        );

        let mut kernel = RecordingKernel::default();
        let left = panel_with_bands(&[(4, true)], millimetres(22));
        build_panel_parts_with(&mut kernel, &left, geometry(&left), EdgeBandProfile::from_spec).expect("band");
        assert_eq!(kernel.frame, Some(Frame { origin: DVec3::new(2.0, 0.0, 0.0), ..Frame::WORLD }));
        let profile = kernel.profile.as_ref().expect("recorded band profile");
        assert_eq!(
            profile.outer,
            vec![DVec2::new(-2.0, 0.0), DVec2::new(0.0, 0.0), DVec2::new(0.0, 500.0), DVec2::new(-2.0, 500.0),]
        );
    }

    #[test]
    fn through_cutout_keeps_its_nominal_position_when_included_bands_move_the_core() {
        let mut kernel = TruckKernel::new();
        let shape = CutoutShape::Rectangle(cutout_rect(10, 10, 10, 10));
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
    fn cutout_tool_stays_at_the_nominal_panel_origin_when_the_core_moves() {
        let mut kernel = RecordingKernel::default();
        let panel =
            panel_with_bands_and_cutout(&[(1, true), (4, true)], CutoutShape::Rectangle(cutout_rect(10, 20, 30, 40)));
        let geometry = geometry(&panel);
        assert_eq!(geometry.core_x(), millimetres(2));
        assert_eq!(geometry.core_y(), millimetres(2));
        let Machining::Cutout(cutout) = &panel.machining()[0];

        build_cutout_tool(&mut kernel, geometry, cutout).expect("cutout tool");
        let frame = kernel.frame.expect("recorded tool frame");
        assert_eq!(frame.origin.x, 10.0);
        assert_eq!(frame.origin.y, 20.0);
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
    fn overlapping_through_cutouts_remove_their_union_volume() {
        let mut kernel = TruckKernel::new();
        let panel = panel_with_cutouts(&[
            (CutoutShape::Rectangle(cutout_rect(100, 100, 100, 50)), CutoutDepth::Through),
            (CutoutShape::Rectangle(cutout_rect(150, 120, 100, 50)), CutoutDepth::Through),
        ]);
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("panel body");

        let measure = kernel.measure(body).expect("panel measure");
        // The rectangles overlap on 50 x 30 mm, so the union removes 8500 mm^2.
        let expected = 6_300_000.0 - 8500.0 * 18.0;
        assert!((measure.volume - expected).abs() < 1.0, "volume was {}, expected {expected}", measure.volume);
    }

    #[test]
    fn a_cutout_across_the_edge_opens_a_notch() {
        let mut kernel = TruckKernel::new();
        let panel =
            panel_with_cutouts(&[(CutoutShape::Rectangle(cutout_rect(650, 10, 100, 50)), CutoutDepth::Through)]);
        let body = build_panel(&mut kernel, &panel, geometry(&panel)).expect("panel body");

        let measure = kernel.measure(body).expect("panel measure");
        // The 700 mm core keeps only the 50 mm of the cutout that reaches inside it.
        let expected = 6_300_000.0 - 50.0 * 50.0 * 18.0;
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
    fn panel_parts_return_no_bands_without_banding() {
        let panel = sample_panel();
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("panel parts");
        assert!(parts.bands().is_empty());
        assert_eq!(kernel.body_count(), 1);
    }
}
