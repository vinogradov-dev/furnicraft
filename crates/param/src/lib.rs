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
    BandingSpec, Cutout, CutoutDepth, CutoutShape, EdgeBanding, EdgeId, FaceSide, Machining, Panel, PanelContourError,
    PanelGeometry, SegmentSource, VertexId,
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
        let body = build_shaped_body_with(kernel, panel, geometry, &EdgeBandProfile::from_spec)?;
        return subtract_machining(kernel, panel, geometry, body);
    }

    let core_x = geometry.core_x().millimetres_f64();
    let core_y = geometry.core_y().millimetres_f64();
    let length = geometry.core_length().millimetres_f64();
    let width = geometry.core_width().millimetres_f64();
    let thickness = geometry.thickness().millimetres_f64();
    let profile = Profile::rectangle(length, width)?;
    let frame = Frame { origin: DVec3::new(core_x, core_y, 0.0), ..Frame::WORLD };
    let body = kernel.extrude(&profile, &frame, thickness)?;
    subtract_machining(kernel, panel, geometry, body)
}

/// Removes every cutout tool from a panel body.
///
/// The same 3D subtraction serves rectangular and shaped cores: tools are nominal panel-local, so
/// they line up with either body without a core-origin translation.
fn subtract_machining(
    kernel: &mut dyn SolidKernel,
    panel: &Panel,
    geometry: PanelGeometry,
    mut body: BodyId,
) -> Result<BodyId, ParamError> {
    for machining in panel.machining() {
        let Machining::Cutout(cutout) = machining;
        let tool = build_cutout_tool(kernel, geometry, cutout)?;
        body = kernel.boolean(BooleanOp::Difference, body, tool)?;
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
        let body = build_shaped_body_with(kernel, panel, geometry, &band_profile)?;
        let body = subtract_machining(kernel, panel, geometry, body)?;
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
/// Each nominal segment is offset inward when its own band is included; consecutive offsets are
/// joined with a straight connector. Unbanded and non-included segments stay nominal.
fn shaped_core_contour<F>(panel: &Panel, band_profile: &F) -> Result<Contour, ParamError>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    Ok(panel.core_contour_with(|spec| band_profile(spec).thickness())?)
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
    let segments = panel.nominal_segments()?;
    let core = panel.core_segments_with(|spec| band_profile(spec).thickness())?;
    let mut parts = Vec::new();
    let mut solids = Vec::new();
    for (order_index, source) in panel.band_order().iter().enumerate() {
        let Some(segment_index) = segments.iter().position(|(_, candidate)| candidate == source) else {
            continue;
        };
        let Some(spec) = panel.segment_banding(*source) else {
            continue;
        };
        let profile = band_profile(&spec);
        let strip =
            shaped_band_strip(panel, &segments, &core, segment_index, spec, profile, order_index, band_profile)?;
        let visible_width = profile.width().min(panel_thickness).millimetres_f64();
        let panel_thickness_mm = panel_thickness.millimetres_f64();
        let z = (panel_thickness_mm - visible_width) * 0.5;
        let frame = Frame { origin: DVec3::new(0.0, 0.0, z), ..Frame::WORLD };
        let body = kernel.extrude(&strip, &frame, visible_width)?;
        solids.push(BandSolid { profile: strip, z, height: visible_width });
        parts.push(BandPart { source: band_source(*source), body });
    }
    trim_overlaps(kernel, &mut parts, &solids)?;
    Ok(parts)
}

/// The planar profile and extrusion of one band, kept for order trimming.
struct BandSolid {
    profile: Profile,
    z: f64,
    height: f64,
}

/// Builds one band strip.
///
/// An included band fills the space between its nominal segment and its core segment, so the body
/// is always cut back by the band's inner face. A non-included band lies outside the nominal
/// outline and is cut flush with the neighbouring faces. A later band still reaches over an earlier
/// neighbouring band's outer face at a sharp joint.
#[allow(clippy::too_many_arguments)]
fn shaped_band_strip<F>(
    panel: &Panel,
    segments: &[(Segment, SegmentSource)],
    core: &[(Segment, SegmentSource)],
    segment_index: usize,
    spec: BandingSpec,
    profile: EdgeBandProfile,
    order_index: usize,
    band_profile: &F,
) -> Result<Profile, ParamError>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let nominal = segments[segment_index].0;
    if spec.included_in_size()
        && let Some((core_segment, _)) = core.get(segment_index)
    {
        if let (Segment::Line(line), Segment::Line(core_line)) = (nominal, *core_segment)
            && let Some(strip) =
                included_line_strip(panel, segments, segment_index, order_index, line, core_line, band_profile)
        {
            return Ok(strip);
        }
        return included_band_profile(nominal, *core_segment);
    }
    if let Segment::Line(line) = nominal
        && let Some(strip) =
            outside_line_strip(panel, segments, segment_index, profile, order_index, line, band_profile)
    {
        return Ok(strip);
    }
    band_strip_profile(&Contour::new(vec![nominal], false)?, spec, profile)
}

/// Fills the gap between a nominal straight segment and its mitred core segment.
#[allow(clippy::too_many_arguments)]
fn included_line_strip<F>(
    panel: &Panel,
    segments: &[(Segment, SegmentSource)],
    segment_index: usize,
    order_index: usize,
    line: Line,
    core_line: Line,
    band_profile: &F,
) -> Option<Profile>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let start = included_end_points(panel, segments, segment_index, false, order_index, line, core_line, band_profile)?;
    let end = included_end_points(panel, segments, segment_index, true, order_index, line, core_line, band_profile)?;
    Profile::new(vec![start.0, end.0, end.1, start.1], Vec::new()).ok()
}

#[allow(clippy::too_many_arguments)]
fn included_end_points<F>(
    panel: &Panel,
    segments: &[(Segment, SegmentSource)],
    segment_index: usize,
    at_end: bool,
    order_index: usize,
    line: Line,
    core_line: Line,
    band_profile: &F,
) -> Option<(DVec2, DVec2)>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let (anchor, along) =
        if at_end { (line.end(), line.end() - line.start()) } else { (line.start(), line.start() - line.end()) };
    if let Some(face) = extension_face(panel, segments, segment_index, at_end, order_index, 0.0, band_profile)
        && let Some(points) = extended_end_points(line, core_line, face, anchor, along)
    {
        return Some(points);
    }
    Some((anchor, if at_end { core_line.end() } else { core_line.start() }))
}

/// Builds a band that lies outside the nominal outline.
#[allow(clippy::too_many_arguments)]
fn outside_line_strip<F>(
    panel: &Panel,
    segments: &[(Segment, SegmentSource)],
    segment_index: usize,
    profile: EdgeBandProfile,
    order_index: usize,
    line: Line,
    band_profile: &F,
) -> Option<Profile>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let thickness = profile.thickness().millimetres_f64();
    let left = line.left_normal();
    let outer = Line::new(line.start() - left * thickness, line.end() - left * thickness).ok()?;
    let start =
        outside_end_points(panel, segments, segment_index, false, order_index, thickness, outer, line, band_profile)?;
    let end =
        outside_end_points(panel, segments, segment_index, true, order_index, thickness, outer, line, band_profile)?;
    Profile::new(vec![start.0, end.0, end.1, start.1], Vec::new()).ok()
}

#[allow(clippy::too_many_arguments)]
fn outside_end_points<F>(
    panel: &Panel,
    segments: &[(Segment, SegmentSource)],
    segment_index: usize,
    at_end: bool,
    order_index: usize,
    outward: f64,
    outer: Line,
    inner: Line,
    band_profile: &F,
) -> Option<(DVec2, DVec2)>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    let line = inner;
    let (anchor, along) =
        if at_end { (line.end(), line.end() - line.start()) } else { (line.start(), line.start() - line.end()) };
    if let Some(face) = extension_face(panel, segments, segment_index, at_end, order_index, outward, band_profile)
        && let Some(points) = extended_end_points(outer, inner, face, anchor, along)
    {
        return Some(points);
    }
    let adjacent_index = if at_end {
        (segment_index + 1) % segments.len()
    } else {
        (segment_index + segments.len() - 1) % segments.len()
    };
    if let Segment::Line(adjacent) = segments[adjacent_index].0
        && let (Some(outer_point), Some(inner_point)) = (outer.intersection(adjacent), inner.intersection(adjacent))
    {
        return Some((outer_point, inner_point));
    }
    Some((if at_end { outer.end() } else { outer.start() }, if at_end { inner.end() } else { inner.start() }))
}

/// The outer face of an earlier neighbouring band that a later band reaches over.
#[allow(clippy::too_many_arguments)]
fn extension_face<F>(
    panel: &Panel,
    segments: &[(Segment, SegmentSource)],
    segment_index: usize,
    at_end: bool,
    order_index: usize,
    outward: f64,
    band_profile: &F,
) -> Option<Line>
where
    F: Fn(&BandingSpec) -> EdgeBandProfile,
{
    if !joint_is_sharp(segments, segment_index, at_end) {
        return None;
    }
    let count = segments.len();
    let adjacent_index = if at_end { (segment_index + 1) % count } else { (segment_index + count - 1) % count };
    let adjacent_source = segments[adjacent_index].1;
    let adjacent_spec = panel.segment_banding(adjacent_source)?;
    let adjacent_order = panel.band_order().iter().position(|candidate| *candidate == adjacent_source)?;
    if adjacent_order >= order_index {
        return None;
    }
    let adjacent_profile = band_profile(&adjacent_spec);
    let adjacent_out =
        if adjacent_spec.included_in_size() { 0.0 } else { adjacent_profile.thickness().millimetres_f64() };
    if outward + 1.0e-9 >= adjacent_out {
        return None;
    }
    let Segment::Line(adjacent_line) = segments[adjacent_index].0 else {
        return None;
    };
    let adjacent_left = adjacent_line.left_normal();
    Line::new(adjacent_line.start() - adjacent_left * adjacent_out, adjacent_line.end() - adjacent_left * adjacent_out)
        .ok()
}

fn extended_end_points(outer: Line, inner: Line, face: Line, anchor: DVec2, along: DVec2) -> Option<(DVec2, DVec2)> {
    let outer_point = outer.intersection(face)?;
    let inner_point = inner.intersection(face)?;
    let outer_beyond = (outer_point - anchor).dot(along);
    let inner_beyond = (inner_point - anchor).dot(along);
    if outer_beyond <= 1.0e-6 || inner_beyond <= 1.0e-6 {
        return None;
    }
    if (outer_point - anchor).length() > 50.0 || (inner_point - anchor).length() > 50.0 {
        return None;
    }
    Some((outer_point, inner_point))
}

/// A band that follows an arc: the nominal curve and its offset bound the strip.
fn included_band_profile(nominal: Segment, core: Segment) -> Result<Profile, ParamError> {
    let outer = Contour::new(vec![nominal], false)?.sample(CONTOUR_SAMPLE_TOLERANCE_MM)?;
    let inner = Contour::new(vec![core], false)?.sample(CONTOUR_SAMPLE_TOLERANCE_MM)?;
    let mut points = outer;
    points.extend(inner.iter().rev().copied());
    Ok(Profile::new(points, Vec::new())?)
}

fn joint_is_sharp(segments: &[(Segment, SegmentSource)], index: usize, at_end: bool) -> bool {
    let count = segments.len();
    let (incoming, outgoing) = if at_end {
        (segments[index].0, segments[(index + 1) % count].0)
    } else {
        (segments[(index + count - 1) % count].0, segments[index].0)
    };
    incoming.tangent_at_end().dot(outgoing.tangent_at_start()).clamp(-1.0, 1.0).acos() > 1.0e-6
}

fn trim_overlaps(kernel: &mut dyn SolidKernel, parts: &mut [BandPart], solids: &[BandSolid]) -> Result<(), ParamError> {
    for later in 1..parts.len() {
        for earlier in 0..later {
            let (Some(later_solid), Some(earlier_solid)) = (solids.get(later), solids.get(earlier)) else {
                continue;
            };
            if !profile_boxes_overlap(&earlier_solid.profile, &later_solid.profile) {
                continue;
            }
            let target = parts[earlier].body;
            if target == parts[later].body {
                continue;
            }
            let frame = Frame { origin: DVec3::new(0.0, 0.0, later_solid.z - TOOL_OVERSHOOT_MM), ..Frame::WORLD };
            let tool = kernel.extrude(&later_solid.profile, &frame, later_solid.height + 2.0 * TOOL_OVERSHOOT_MM)?;
            match kernel.boolean(BooleanOp::Difference, target, tool) {
                Ok(body) => parts[earlier].body = body,
                Err(_) => {
                    // The boolean may consume the target even when it finds no solid to keep.
                    // Rebuild the band so the caller still gets a valid body.
                    let frame = Frame { origin: DVec3::new(0.0, 0.0, earlier_solid.z), ..Frame::WORLD };
                    parts[earlier].body = kernel.extrude(&earlier_solid.profile, &frame, earlier_solid.height)?;
                }
            }
        }
    }
    Ok(())
}

fn profile_boxes_overlap(a: &Profile, b: &Profile) -> bool {
    let bounds = |profile: &Profile| {
        profile
            .outer
            .iter()
            .fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(min, max), point| {
                (min.min(*point), max.max(*point))
            })
    };
    let (a_min, a_max) = bounds(a);
    let (b_min, b_max) = bounds(b);
    a_min.x < b_max.x - 1.0e-6 && b_min.x < a_max.x - 1.0e-6 && a_min.y < b_max.y - 1.0e-6 && b_min.y < a_max.y - 1.0e-6
}

fn band_source(source: SegmentSource) -> BandSource {
    match source {
        SegmentSource::Edge(edge) => BandSource::Edge(edge),
        SegmentSource::Corner(corner) => BandSource::Corner(corner),
    }
}

fn band_strip_profile(path: &Contour, spec: BandingSpec, profile: EdgeBandProfile) -> Result<Profile, ParamError> {
    let distance = profile.thickness().millimetres_f64();
    let (outer, inner) = if spec.included_in_size() {
        (path.clone(), path.offset(distance)?)
    } else {
        (path.offset(-distance)?, path.clone())
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
        profiles: Vec<Profile>,
        frame: Option<Frame>,
        height: Option<f64>,
        fail: bool,
    }

    impl SolidKernel for RecordingKernel {
        fn extrude(&mut self, profile: &Profile, frame: &Frame, height: f64) -> furnicraft_kernel::Result<BodyId> {
            self.profile = Some(profile.clone());
            self.profiles.push(profile.clone());
            self.frame = Some(*frame);
            self.height = Some(height);
            if self.fail {
                return Err(KernelError::Failed("recording kernel was asked to fail".into()));
            }
            Ok(BodyId::from_raw(7))
        }

        fn boolean(&mut self, _op: BooleanOp, target: BodyId, _tool: BodyId) -> furnicraft_kernel::Result<BodyId> {
            Ok(target)
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
            self.profiles.clear();
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

    fn panel_with_radius_and_cutout(cutout: Option<CutoutShape>) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        if let Some(shape) = cutout {
            project.add_cutout(panel_id, shape, CutoutDepth::Through, FaceSide::A).expect("add cutout");
        }
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(RectCorner::BottomRight.id(), CornerFeatureKind::Radius { radius: millimetres(25) })
                    .expect("radius"),
            )
            .expect("set corner");
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

    fn panel_with_bottom_band_and_chamfer_corner() -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(RectEdge::Bottom.id(), edge_material(), false, millimetres(2), millimetres(22))
                    .expect("band"),
            )
            .expect("band the bottom edge");
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
            .expect("set chamfer");
        project
            .set_corner_banding(
                panel_id,
                CornerBanding::new(corner, edge_material(), true, millimetres(2), millimetres(22))
                    .expect("corner band"),
            )
            .expect("band the chamfer");
        project.panel(panel_id).expect("panel").clone()
    }

    fn panel_with_chamfer_and_end_bands(
        bottom_included: bool,
        right_included: bool,
        chamfer_included: Option<bool>,
    ) -> Panel {
        let root = Assembly::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
        let mut project = Project::new(root).expect("valid project");
        let panel_id = project
            .add_panel(project.root().id(), "Panel", panel_size(), None, millimetres(18), Placement::IDENTITY)
            .expect("add panel");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(
                    RectEdge::Bottom.id(),
                    edge_material(),
                    bottom_included,
                    millimetres(2),
                    millimetres(22),
                )
                .expect("bottom band"),
            )
            .expect("band the bottom edge");
        project
            .set_edge_banding(
                panel_id,
                EdgeBanding::new(
                    RectEdge::Right.id(),
                    edge_material(),
                    right_included,
                    millimetres(2),
                    millimetres(22),
                )
                .expect("right band"),
            )
            .expect("band the right edge");
        let corner = RectCorner::BottomRight.id();
        project
            .set_corner_feature(
                panel_id,
                CornerFeature::new(
                    corner,
                    CornerFeatureKind::Chamfer { incoming: millimetres(20), outgoing: millimetres(5) },
                )
                .expect("chamfer"),
            )
            .expect("set chamfer");
        if let Some(included) = chamfer_included {
            project
                .set_corner_banding(
                    panel_id,
                    CornerBanding::new(corner, edge_material(), included, millimetres(2), millimetres(22))
                        .expect("corner band"),
                )
                .expect("band the chamfer");
        }
        project.panel(panel_id).expect("panel").clone()
    }

    fn assert_profile_point(profile: &Profile, x: f64, y: f64) {
        assert!(
            profile.outer.iter().any(|point| (*point - DVec2::new(x, y)).length() < 1.0e-6),
            "expected ({x}, {y}) in {:?}",
            profile.outer
        );
    }

    #[test]
    fn included_end_bands_fill_a_bare_chamfer_join() {
        let panel = panel_with_chamfer_and_end_bands(true, true, None);
        let mut kernel = RecordingKernel::default();
        let parts = build_shaped_bands(&mut kernel, &panel, geometry(&panel), &EdgeBandProfile::from_spec)
            .expect("shaped bands");
        assert_eq!(parts.len(), 2);
        assert_eq!(kernel.profiles.len(), 2);
        let bottom = &kernel.profiles[0];
        let right = &kernel.profiles[1];
        assert_profile_point(bottom, 680.0, 0.0);
        assert_profile_point(bottom, 688.0, 2.0);
        assert_profile_point(right, 700.0, 5.0);
        assert_profile_point(right, 698.0, 4.5);
    }

    #[test]
    fn non_included_end_bands_are_cut_flush_with_a_bare_chamfer() {
        let panel = panel_with_chamfer_and_end_bands(false, false, None);
        let mut kernel = RecordingKernel::default();
        let parts = build_shaped_bands(&mut kernel, &panel, geometry(&panel), &EdgeBandProfile::from_spec)
            .expect("shaped bands");
        assert_eq!(parts.len(), 2);
        let bottom = &kernel.profiles[0];
        let right = &kernel.profiles[1];
        assert_profile_point(bottom, 672.0, -2.0);
        assert_profile_point(bottom, 680.0, 0.0);
        assert_profile_point(right, 700.0, 5.0);
        assert_profile_point(right, 702.0, 5.5);
    }

    #[test]
    fn included_bands_meet_a_banded_chamfer_without_gaps_or_overlaps() {
        let panel = panel_with_chamfer_and_end_bands(true, true, Some(true));
        let nominal_volume = panel.nominal_contour().expect("contour").signed_area().expect("area") * 18.0;
        let mut kernel = TruckKernel::new();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("banded chamfer parts");
        let body_volume = kernel.measure(parts.body()).expect("body measure").volume;
        let band_volume: f64 =
            parts.bands().iter().map(|part| kernel.measure(part.body()).expect("band measure").volume).sum();
        assert!(
            (body_volume + band_volume - nominal_volume).abs() < nominal_volume * 1.0e-3,
            "body {body_volume} + bands {band_volume} vs nominal {nominal_volume}"
        );
        kernel.clear();
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
    fn an_edge_band_produces_one_strip_on_its_segment() {
        let panel = panel_with_radius_and_band(false);
        assert_eq!(panel.edge_banding().len(), 1);
        let mut kernel = RecordingKernel::default();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("rounded panel parts");
        assert_eq!(parts.bands().len(), 1);
        assert_eq!(parts.bands()[0].source(), BandSource::Edge(RectEdge::Right.id()));
    }

    #[test]
    fn a_later_chamfer_band_reaches_an_earlier_edge_band_outer_face() {
        let panel = panel_with_bottom_band_and_chamfer_corner();
        assert_eq!(
            panel.band_order(),
            &[SegmentSource::Edge(RectEdge::Bottom.id()), SegmentSource::Corner(RectCorner::BottomRight.id())]
        );
        let mut kernel = RecordingKernel::default();
        let parts = build_panel_parts_with(&mut kernel, &panel, geometry(&panel), EdgeBandProfile::from_spec)
            .expect("panel parts");
        assert_eq!(parts.bands().len(), 2);
        let strip = kernel.profile.as_ref().expect("the chamfer strip is the last extrusion");
        let min_y = strip.outer.iter().map(|point| point.y).fold(f64::INFINITY, f64::min);
        assert!(min_y <= -1.99, "the chamfer band must reach the bottom band outer face, min y {min_y}");
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
    fn shaped_panel_subtracts_cutouts() {
        let mut kernel = TruckKernel::new();
        let shape = CutoutShape::Rectangle(cutout_rect(100, 100, 100, 50));
        let shaped = panel_with_radius_and_cutout(Some(shape));
        let body = build_panel(&mut kernel, &shaped, geometry(&shaped)).expect("shaped body with cutout");
        let with_cutout = kernel.measure(body).expect("shaped measure").volume;

        let reference = panel_with_radius_and_cutout(None);
        let reference_body = build_panel(&mut kernel, &reference, geometry(&reference)).expect("shaped reference");
        let without_cutout = kernel.measure(reference_body).expect("reference measure").volume;

        let removed = without_cutout - with_cutout;
        let expected = 100.0 * 50.0 * 18.0;
        assert!((removed - expected).abs() < 1.0, "removed {removed}, expected {expected}");
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
