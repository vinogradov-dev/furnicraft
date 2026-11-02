//! The nominal panel outline: a rectangle with corner treatments and bowed edges.
//!
//! The outline is counter-clockwise and starts on edge 1 after corner 1. Edge `i` runs from corner
//! `i` to corner `i + 1`; a corner treatment is inserted after the edge that ends there. The builder
//! keeps the document free of kernel types and leaves core/outer offsets to the parametric layer.

use std::f64::consts::FRAC_PI_2;

use furnicraft_geom::{Arc, Contour, ContourError, DEFAULT_TANGENT_TOLERANCE, DVec2, Line, Segment};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    BandingSpec, ContourSegmentId, CornerFeature, CornerFeatureKind, EdgeBanding, EdgeId, Length, Panel,
    PanelNominalSize, RectCorner, RectEdge, RingBanding, VertexId,
};

/// The ring of a contour panel as the nominal outline, with one source per segment.
///
/// The stored ring is already the finished shape, so nothing is offset or rebuilt here. The size is
/// re-derived rather than trusted: a resize candidate that no longer matches the ring is an error,
/// because a contour panel is sized by its ring and cannot be stretched to a rectangle.
fn ring_segments(
    ring: &crate::Contour,
    nominal_size: PanelNominalSize,
) -> Result<(Contour, Vec<SegmentSource>), PanelContourError> {
    let (min, max) = ring.bounds_micrometres();
    let length = max[0].saturating_sub(min[0]);
    let width = max[1].saturating_sub(min[1]);
    if min != [0, 0] || length != nominal_size.length().micrometres() || width != nominal_size.width().micrometres() {
        return Err(PanelContourError::RingSizeMismatch { start: min, length, width });
    }
    let mut segments = Vec::with_capacity(ring.entries().len());
    let mut sources = Vec::with_capacity(ring.entries().len());
    for entry in ring.entries() {
        segments.push(entry.segment().to_geom()?);
        sources.push(SegmentSource::Ring(entry.id()));
    }
    Ok((Contour::new(segments, true)?, sources))
}

/// Why a panel's nominal contour cannot be built.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PanelContourError {
    /// A line, arc or closed contour is not geometrically valid.
    #[error(transparent)]
    Geometry(#[from] ContourError),
    /// A stored ring segment cannot become a geometry segment.
    #[error(transparent)]
    Segment(#[from] crate::ContourSegmentError),
    /// Corner treatments consume the whole edge between them.
    #[error("corner features leave no straight edge on edge {edge}")]
    FeatureOverlap {
        /// The edge whose clear length is not positive.
        edge: EdgeId,
    },
    /// A bow is deeper than half of its clear chord.
    #[error("bow on edge {edge} is too deep for its clear edge")]
    BowTooDeep {
        /// The bowed edge.
        edge: EdgeId,
    },
    /// A corner treatment produced endpoints that do not meet its neighbours.
    #[error("corner feature geometry is inconsistent")]
    FeatureGeometry,
    /// The contour has no finite bounds.
    #[error("panel contour bounds are not finite")]
    InvalidBounds,
    /// The requested edge is not part of the nominal outline.
    #[error("edge {edge} is not part of the panel outline")]
    UnsupportedEdge {
        /// The rejected edge.
        edge: EdgeId,
    },
    /// The requested ring segment is not part of the panel outline.
    #[error("segment {segment} is not part of the panel outline")]
    UnsupportedRingSegment {
        /// The rejected ring segment.
        segment: ContourSegmentId,
    },
    /// A contour panel's stored ring no longer matches the nominal size it stores.
    #[error("the contour panel ring starts at {start:?} micrometres and spans {length} by {width} micrometres")]
    RingSizeMismatch {
        /// The ring's minimum corner, in micrometres.
        start: [i64; 2],
        /// The ring's extent along X, in micrometres.
        length: i64,
        /// The ring's extent along Y, in micrometres.
        width: i64,
    },
}

/// Where one nominal contour segment comes from.
///
/// The same value identifies a banded segment in [`Panel::band_order`], so it is serialized with the
/// panel. A rectangular outline names its four edges and its corner treatments; a contour ring
/// names the ring segment itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentSource {
    /// A straight or bowed edge of the nominal rectangle.
    Edge(EdgeId),
    /// A corner radius or chamfer of the nominal rectangle.
    Corner(VertexId),
    /// One segment of a contour ring, addressed by its stable id inside that ring.
    Ring(ContourSegmentId),
}

/// A maximal tangent-continuous chain of nominal contour segments.
///
/// The chain ends at sharp joints. An unbanded rectangular panel therefore has four paths, one per
/// edge; a rounded corner joins its two adjacent edges into one path.
#[derive(Clone, Debug, PartialEq)]
pub struct ContourPath {
    contour: Contour,
    sources: Vec<SegmentSource>,
}

impl ContourPath {
    /// The path geometry.
    pub const fn contour(&self) -> &Contour {
        &self.contour
    }

    /// The source of each path segment, in order.
    pub fn sources(&self) -> &[SegmentSource] {
        &self.sources
    }

    /// Whether the path closes on itself.
    pub const fn is_closed(&self) -> bool {
        self.contour.is_closed()
    }

    /// The distinct edges touched by this path, in path order.
    pub fn edge_ids(&self) -> impl Iterator<Item = EdgeId> + '_ {
        let mut seen = Vec::new();
        self.sources.iter().filter_map(move |source| match source {
            SegmentSource::Edge(edge) if !seen.contains(edge) => {
                seen.push(*edge);
                Some(*edge)
            }
            _ => None,
        })
    }

    /// The corner source when the path is a lone chamfer or radius segment.
    pub fn lone_corner(&self) -> Option<VertexId> {
        match self.sources.as_slice() {
            [SegmentSource::Corner(corner)] => Some(*corner),
            _ => None,
        }
    }
}

impl Panel {
    /// Builds the nominal outline in panel-local millimetres.
    ///
    /// Positive bow sagitta is outward. The outline is counter-clockwise and its bounds are the
    /// nominal rectangle extended by outward bows; a radius or chamfer does not move the rectangle
    /// bounds unless it consumes a whole side, which is rejected.
    ///
    /// A contour panel has no rectangle to build: its ring *is* the outline, so this converts the
    /// stored ring to millimetres unchanged.
    pub fn nominal_contour(&self) -> Result<Contour, PanelContourError> {
        self.nominal_contour_for(self.nominal_size())
    }

    /// Builds the nominal outline for a candidate size, used by resize validation.
    pub fn nominal_contour_for(&self, nominal_size: PanelNominalSize) -> Result<Contour, PanelContourError> {
        self.contour_with_sources_for(nominal_size).map(|(contour, _)| contour)
    }

    /// Groups the nominal outline into maximal tangent-continuous paths.
    pub fn contour_paths(&self) -> Result<Vec<ContourPath>, PanelContourError> {
        self.contour_paths_for(self.nominal_size())
    }

    /// Groups a candidate nominal outline, used by resize validation.
    pub fn contour_paths_for(&self, nominal_size: PanelNominalSize) -> Result<Vec<ContourPath>, PanelContourError> {
        let (contour, sources) = self.contour_with_sources_for(nominal_size)?;
        let segments = contour.segments();
        let segment_count = segments.len();
        let mut starts = Vec::new();
        for index in 0..segment_count {
            if contour.joint_is_smooth(index, DEFAULT_TANGENT_TOLERANCE) == Some(false) {
                starts.push((index + 1) % segment_count);
            }
        }
        if starts.is_empty() {
            return Ok(vec![ContourPath { contour, sources }]);
        }

        let mut paths = Vec::with_capacity(starts.len());
        for start in starts {
            let mut path_segments = Vec::new();
            let mut path_sources = Vec::new();
            let mut index = start;
            loop {
                path_segments.push(*segments.get(index).ok_or(PanelContourError::FeatureGeometry)?);
                path_sources.push(*sources.get(index).ok_or(PanelContourError::FeatureGeometry)?);
                let smooth = contour
                    .joint_is_smooth(index, DEFAULT_TANGENT_TOLERANCE)
                    .ok_or(PanelContourError::FeatureGeometry)?;
                if !smooth {
                    break;
                }
                index = (index + 1) % segment_count;
                if index == start {
                    // A closed path with no sharp joint is handled above; guard the walk anyway.
                    break;
                }
            }
            paths.push(ContourPath { contour: Contour::new(path_segments, false)?, sources: path_sources });
        }
        Ok(paths)
    }

    /// Finds the tangent-continuous path that contains an edge.
    pub fn path_containing_edge(&self, edge: EdgeId) -> Result<Option<ContourPath>, PanelContourError> {
        Ok(self.contour_paths()?.into_iter().find(|path| path.edge_ids().any(|candidate| candidate == edge)))
    }

    /// Finds the tangent-continuous path that contains a ring segment.
    pub fn path_containing_ring_segment(
        &self,
        segment: ContourSegmentId,
    ) -> Result<Option<ContourPath>, PanelContourError> {
        Ok(self.contour_paths()?.into_iter().find(|path| path.sources().contains(&SegmentSource::Ring(segment))))
    }

    /// Adds or replaces banding on one edge segment.
    ///
    /// A banding belongs to its segment; neighbouring segments of the same tangent path keep their
    /// own records.
    pub fn apply_edge_banding(&mut self, band: EdgeBanding) -> Result<(), PanelContourError> {
        let edge = band.edge();
        self.path_containing_edge(edge)?.ok_or(PanelContourError::UnsupportedEdge { edge })?;
        self.edge_banding.retain(|existing| existing.edge() != edge);
        self.edge_banding.push(band);
        Ok(())
    }

    /// Removes banding from one edge segment.
    pub fn clear_edge_banding_path(&mut self, edge: EdgeId) -> Result<bool, PanelContourError> {
        self.path_containing_edge(edge)?.ok_or(PanelContourError::UnsupportedEdge { edge })?;
        let before = self.edge_banding.len();
        self.edge_banding.retain(|existing| existing.edge() != edge);
        Ok(self.edge_banding.len() != before)
    }

    /// Adds or replaces banding on one segment of a contour ring.
    ///
    /// A ring segment is already its own segment, so nothing is grouped or copied to neighbours: one
    /// record belongs to one [`ContourSegmentId`] and disappears with it when the outline is edited.
    pub fn apply_ring_banding(&mut self, band: RingBanding) -> Result<(), PanelContourError> {
        let segment = band.segment();
        if self.outline_contour().is_none() || self.path_containing_ring_segment(segment)?.is_none() {
            return Err(PanelContourError::UnsupportedRingSegment { segment });
        }
        self.ring_banding.retain(|existing| existing.segment() != segment);
        self.ring_banding.push(band);
        Ok(())
    }

    /// Removes banding from one ring segment.
    pub fn clear_ring_banding(&mut self, segment: ContourSegmentId) -> Result<bool, PanelContourError> {
        if self.outline_contour().is_none() || self.path_containing_ring_segment(segment)?.is_none() {
            return Err(PanelContourError::UnsupportedRingSegment { segment });
        }
        let before = self.ring_banding.len();
        self.ring_banding.retain(|existing| existing.segment() != segment);
        Ok(self.ring_banding.len() != before)
    }

    /// The banding on one contour segment, if any.
    pub fn segment_banding(&self, source: SegmentSource) -> Option<BandingSpec> {
        match source {
            SegmentSource::Edge(edge) => {
                self.edge_banding.iter().find(|band| band.edge() == edge).map(EdgeBanding::spec)
            }
            SegmentSource::Corner(corner) => self.corner_banding_at(corner).map(|band| band.spec()),
            SegmentSource::Ring(segment) => self.ring_banding_at(segment).map(|band| band.spec()),
        }
    }

    /// The nominal segments in travel order, with their source.
    pub fn nominal_segments(&self) -> Result<Vec<(Segment, SegmentSource)>, PanelContourError> {
        self.nominal_segments_for(self.nominal_size())
    }

    /// The nominal segments for a candidate size, in travel order.
    pub fn nominal_segments_for(
        &self,
        nominal_size: PanelNominalSize,
    ) -> Result<Vec<(Segment, SegmentSource)>, PanelContourError> {
        let mut segments = Vec::new();
        for path in self.contour_paths_for(nominal_size)? {
            for (segment, source) in path.contour().segments().iter().zip(path.sources()) {
                segments.push((*segment, *source));
            }
        }
        Ok(segments)
    }

    /// The core outline segment for every nominal segment.
    ///
    /// Each nominal segment is offset inward by its own included band thickness. A joint between two
    /// straight segments is mitred at the intersection of their offset lines, so a band's inner face
    /// bounds the core and the next face's body cannot cover the band. Arc joints keep their offset
    /// endpoints and are joined with a straight connector by [`Self::core_contour_with`].
    pub fn core_segments_with<F>(&self, band_thickness: F) -> Result<Vec<(Segment, SegmentSource)>, PanelContourError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        self.core_segments_for_with(self.nominal_size(), band_thickness)
    }

    /// The core outline segments for a candidate size and band thickness.
    pub fn core_segments_for_with<F>(
        &self,
        nominal_size: PanelNominalSize,
        band_thickness: F,
    ) -> Result<Vec<(Segment, SegmentSource)>, PanelContourError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        let nominal = self.nominal_segments_for(nominal_size)?;
        let mut pieces = Vec::with_capacity(nominal.len());
        for (segment, source) in &nominal {
            let distance = self
                .segment_banding(*source)
                .filter(|spec| spec.included_in_size())
                .map_or(0.0, |spec| band_thickness(&spec).millimetres_f64());
            let piece = if distance > 0.0 { segment.offset(distance)? } else { *segment };
            pieces.push((piece, *source));
        }
        let nominal_segments = nominal.iter().map(|(segment, _)| *segment).collect();
        let nominal_contour = Contour::new(nominal_segments, true)?;
        let (min, max) = nominal_contour.bounds().ok_or(ContourError::InvalidInput("panel outline bounds"))?;
        trim_straight_joints(&mut pieces, min, max);
        Ok(pieces)
    }

    /// Builds the core outline with a per-segment offset for included banding.
    ///
    /// Straight joints are mitred; see [`Self::core_segments_with`]. Segments that still do not meet
    /// (arc joints and degenerate mitres) are joined with a straight connector, so a band on one
    /// segment does not move its neighbours.
    pub fn core_contour_with<F>(&self, band_thickness: F) -> Result<Contour, PanelContourError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        self.core_contour_for_with(self.nominal_size(), band_thickness)
    }

    /// Builds the core outline for a candidate size and band thickness.
    pub fn core_contour_for_with<F>(
        &self,
        nominal_size: PanelNominalSize,
        band_thickness: F,
    ) -> Result<Contour, PanelContourError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        let pieces = self.core_segments_for_with(nominal_size, band_thickness)?;
        join_segments(pieces.into_iter().map(|(segment, _)| segment).collect())
    }

    /// The bounds of the outer envelope with a per-segment outward offset for non-included banding.
    pub fn outer_bounds_with<F>(&self, band_thickness: F) -> Result<(DVec2, DVec2), PanelContourError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        self.outer_bounds_for_with(self.nominal_size(), band_thickness)
    }

    /// The outer envelope bounds for a candidate size and band thickness.
    pub fn outer_bounds_for_with<F>(
        &self,
        nominal_size: PanelNominalSize,
        band_thickness: F,
    ) -> Result<(DVec2, DVec2), PanelContourError>
    where
        F: Fn(&BandingSpec) -> Length,
    {
        let mut min = DVec2::splat(f64::INFINITY);
        let mut max = DVec2::splat(f64::NEG_INFINITY);
        for (segment, source) in self.nominal_segments_for(nominal_size)? {
            let distance = self
                .segment_banding(source)
                .filter(|spec| !spec.included_in_size())
                .map_or(0.0, |spec| band_thickness(&spec).millimetres_f64());
            let bound = if distance > 0.0 { segment.offset(-distance)? } else { segment };
            let (segment_min, segment_max) = bound.bounds();
            min = min.min(segment_min);
            max = max.max(segment_max);
        }
        if !min.is_finite() || !max.is_finite() {
            return Err(PanelContourError::InvalidBounds);
        }
        Ok((min, max))
    }

    /// The banding that currently covers a path, if any.
    ///
    /// A lone chamfer uses its corner record; every other path uses the records on its edges. If a
    /// loaded file left a path partially banded or with differing specs, the first record wins here;
    /// [`Self::path_banding_conflict`] reports the inconsistency for command validation.
    pub fn path_banding(&self, path: &ContourPath) -> Option<BandingSpec> {
        if let Some(corner) = path.lone_corner() {
            return self.corner_banding_at(corner).map(|band| band.spec());
        }
        path.edge_ids()
            .find_map(|edge| self.edge_banding.iter().find(|band| band.edge() == edge).map(EdgeBanding::spec))
    }

    /// Whether any path is partially banded or carries conflicting banding specs.
    pub fn has_path_banding_conflict(&self) -> Result<bool, PanelContourError> {
        for path in self.contour_paths()? {
            if self.path_banding_conflict(&path) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether a path has banding on some but not all of its edges, or conflicting specs.
    ///
    /// A ring path has no edges to disagree: each ring segment owns its own record, so it never
    /// carries a conflict.
    pub fn path_banding_conflict(&self, path: &ContourPath) -> bool {
        if path.lone_corner().is_some() || path.sources().iter().any(|source| matches!(source, SegmentSource::Ring(_)))
        {
            return false;
        }
        let mut seen = None;
        for edge in path.edge_ids() {
            let Some(band) = self.edge_banding.iter().find(|band| band.edge() == edge) else {
                return seen.is_some();
            };
            let spec = band.spec();
            match seen {
                Some(existing) if existing != spec => return true,
                Some(_) => {}
                None => seen = Some(spec),
            }
        }
        false
    }

    fn contour_with_sources_for(
        &self,
        nominal_size: PanelNominalSize,
    ) -> Result<(Contour, Vec<SegmentSource>), PanelContourError> {
        if let Some(ring) = self.outline_contour() {
            return ring_segments(ring, nominal_size);
        }
        let length = nominal_size.length().millimetres_f64();
        let width = nominal_size.width().millimetres_f64();
        let corners =
            [DVec2::new(0.0, 0.0), DVec2::new(length, 0.0), DVec2::new(length, width), DVec2::new(0.0, width)];
        let directions = [DVec2::X, DVec2::Y, DVec2::NEG_X, DVec2::NEG_Y];
        let edge_lengths = [length, width, length, width];

        let mut segments = Vec::with_capacity(8);
        let mut sources = Vec::with_capacity(8);
        for index in 0..4 {
            let edge = RectEdge::ALL[index];
            let edge_id = edge.id();
            let start_corner = RectCorner::ALL[index];
            let end_corner = RectCorner::ALL[(index + 1) % 4];
            let direction = directions[index];
            let start_setback = self
                .corner_feature(start_corner.id())
                .map(|feature| outgoing_setback(feature.feature()))
                .unwrap_or(0.0);
            let end_setback =
                self.corner_feature(end_corner.id()).map(|feature| incoming_setback(feature.feature())).unwrap_or(0.0);
            let clear = edge_lengths[index] - start_setback - end_setback;
            if !clear.is_finite() || clear <= 1.0e-6 {
                return Err(PanelContourError::FeatureOverlap { edge: edge_id });
            }

            let start = corners[index] + direction * start_setback;
            let end = corners[(index + 1) % 4] - direction * end_setback;
            let edge_segment = match self.edge_bow(edge_id) {
                Some(bow) => {
                    // The contour is counter-clockwise, so its left side is the material side.
                    // A positive document sagitta bows outward, which is to the right of travel.
                    let sagitta = -bow.sagitta().millimetres_f64();
                    match Arc::from_endpoints_sagitta(start, end, sagitta) {
                        Ok(arc) => Segment::Arc(arc),
                        Err(ContourError::SagittaTooDeep) => {
                            return Err(PanelContourError::BowTooDeep { edge: edge_id });
                        }
                        Err(error) => return Err(PanelContourError::Geometry(error)),
                    }
                }
                None => Segment::Line(Line::new(start, end)?),
            };
            segments.push(edge_segment);
            sources.push(SegmentSource::Edge(edge_id));

            if let Some(feature) = self.corner_feature(end_corner.id()) {
                segments.push(corner_segment(
                    feature,
                    end,
                    corners[(index + 1) % 4],
                    directions[index],
                    directions[(index + 1) % 4],
                )?);
                sources.push(SegmentSource::Corner(end_corner.id()));
            }
        }

        let contour = Contour::new(segments, true)?;
        if contour.is_counter_clockwise() != Some(true) {
            return Err(PanelContourError::FeatureGeometry);
        }
        Ok((contour, sources))
    }
}

fn corner_segment(
    feature: CornerFeature,
    incoming_point: DVec2,
    corner: DVec2,
    incoming_direction: DVec2,
    outgoing_direction: DVec2,
) -> Result<Segment, PanelContourError> {
    match feature.feature() {
        CornerFeatureKind::Radius { radius } => {
            let radius = radius.millimetres_f64();
            let outgoing_point = corner + outgoing_direction * radius;
            let left = DVec2::new(-incoming_direction.y, incoming_direction.x);
            // For a counter-clockwise 90-degree corner the arc center is one radius inside the
            // incoming edge from its tangent point.
            let center = incoming_point + left * radius;
            let radial = incoming_point - center;
            let start_angle = radial.y.atan2(radial.x);
            let arc = Arc::new(center, radius, start_angle, FRAC_PI_2)?;
            if (arc.end() - outgoing_point).length() > 1.0e-6 {
                return Err(PanelContourError::FeatureGeometry);
            }
            Ok(Segment::Arc(arc))
        }
        CornerFeatureKind::Chamfer { outgoing, .. } => {
            let outgoing_point = corner + outgoing_direction * outgoing.millimetres_f64();
            Ok(Segment::Line(Line::new(incoming_point, outgoing_point)?))
        }
    }
}

fn outgoing_setback(feature: CornerFeatureKind) -> f64 {
    match feature {
        CornerFeatureKind::Radius { radius } => radius.millimetres_f64(),
        CornerFeatureKind::Chamfer { outgoing, .. } => outgoing.millimetres_f64(),
    }
}

fn incoming_setback(feature: CornerFeatureKind) -> f64 {
    match feature {
        CornerFeatureKind::Radius { radius } => radius.millimetres_f64(),
        CornerFeatureKind::Chamfer { incoming, .. } => incoming.millimetres_f64(),
    }
}

/// Mitres consecutive straight offset segments at the intersection of their supporting lines.
///
/// The intersection is only used when it lies inside the nominal outline and both trimmed pieces
/// keep their travel direction: a band thicker than the feature it meets is not allowed to fold the
/// core back on itself. A rejected mitre keeps the untouched offset endpoints and is joined by
/// [`join_segments`].
fn trim_straight_joints(pieces: &mut [(Segment, SegmentSource)], min: DVec2, max: DVec2) {
    let count = pieces.len();
    let tolerance = 1.0e-6;
    for index in 0..count {
        let next = (index + 1) % count;
        let (Segment::Line(current), Segment::Line(neighbour)) = (pieces[index].0, pieces[next].0) else {
            continue;
        };
        let Some(meeting) = current.intersection(neighbour) else {
            continue;
        };
        if meeting.x < min.x - tolerance
            || meeting.x > max.x + tolerance
            || meeting.y < min.y - tolerance
            || meeting.y > max.y + tolerance
        {
            continue;
        }
        let Ok(trimmed_current) = Line::new(current.start(), meeting) else {
            continue;
        };
        let Ok(trimmed_neighbour) = Line::new(meeting, neighbour.end()) else {
            continue;
        };
        if trimmed_current.length() <= tolerance || trimmed_neighbour.length() <= tolerance {
            continue;
        }
        let current_forward = (current.end() - current.start()).dot(trimmed_current.end() - trimmed_current.start());
        let neighbour_forward =
            (neighbour.end() - neighbour.start()).dot(trimmed_neighbour.end() - trimmed_neighbour.start());
        if current_forward <= 0.0 || neighbour_forward <= 0.0 {
            continue;
        }
        pieces[index].0 = Segment::Line(trimmed_current);
        pieces[next].0 = Segment::Line(trimmed_neighbour);
    }
}

/// Closes per-segment offset segments into one contour, adding a straight connector at a step.
fn join_segments(segments: Vec<Segment>) -> Result<Contour, PanelContourError> {
    let Some(first) = segments.first().copied() else {
        return Err(ContourError::Empty.into());
    };
    let mut joined = Vec::with_capacity(segments.len() + 4);
    let mut previous_end: Option<DVec2> = None;
    for segment in &segments {
        if let Some(end) = previous_end
            && end.distance(segment.start()) > 1.0e-6
        {
            joined.push(Segment::Line(Line::new(end, segment.start())?));
        }
        joined.push(*segment);
        previous_end = Some(segment.end());
    }
    if let Some(end) = previous_end
        && end.distance(first.start()) > 1.0e-6
    {
        joined.push(Segment::Line(Line::new(end, first.start())?));
    }
    Ok(Contour::new(joined, true)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::{EdgeBow, Id, MaterialId, PanelNominalSize, Placement, RectCorner, RectEdge};
    use furnicraft_units::Length;

    fn millimetres(value: i64) -> Length {
        Length::from_millimetres(value).expect("whole millimetres fit a length")
    }

    fn panel(length: i64, width: i64) -> Panel {
        Panel::new(
            Id::new(7).expect("panel id"),
            "panel",
            PanelNominalSize::new(millimetres(length), millimetres(width)).expect("nominal size"),
            None,
            millimetres(18),
            Placement::IDENTITY,
        )
        .expect("panel")
    }

    fn area_after_radius(panel: &Panel) -> f64 {
        let contour = panel.nominal_contour().expect("contour");
        contour.signed_area().expect("area")
    }

    #[test]
    fn plain_panel_is_the_nominal_rectangle() {
        let panel = panel(100, 60);
        let contour = panel.nominal_contour().expect("contour");
        assert_eq!(contour.segments().len(), 4);
        assert_eq!(contour.signed_area().expect("area"), 6000.0);
        assert_eq!(contour.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 60.0))));
    }

    #[test]
    fn radius_replaces_one_corner_and_keeps_tangency() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(10) })
                .expect("radius"),
        );
        let contour = panel.nominal_contour().expect("contour");
        assert_eq!(contour.segments().len(), 5);
        assert_eq!(contour.bounds(), Some((DVec2::ZERO, DVec2::new(100.0, 60.0))));
        let area = area_after_radius(&panel);
        let expected = 6000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 100.0;
        assert!((area - expected).abs() < 1e-9, "area {area}, expected {expected}");
    }

    #[test]
    fn chamfer_uses_independent_setbacks() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(
                RectCorner::BottomRight.id(),
                CornerFeatureKind::Chamfer { incoming: millimetres(20), outgoing: millimetres(5) },
            )
            .expect("chamfer"),
        );
        let contour = panel.nominal_contour().expect("contour");
        assert_eq!(contour.segments().len(), 5);
        let expected = 6000.0 - 0.5 * 20.0 * 5.0;
        assert!((contour.signed_area().expect("area") - expected).abs() < 1e-9);
    }

    #[test]
    fn an_included_band_cuts_the_core_at_a_bare_chamfer() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(
                RectCorner::BottomRight.id(),
                CornerFeatureKind::Chamfer { incoming: millimetres(20), outgoing: millimetres(5) },
            )
            .expect("chamfer"),
        );
        let material = MaterialId::new(1).expect("material");
        for edge in [RectEdge::Bottom, RectEdge::Right] {
            panel
                .apply_edge_banding(
                    EdgeBanding::new(edge.id(), material, true, millimetres(2), millimetres(20)).expect("band"),
                )
                .expect("apply band");
        }

        let core = panel.core_contour_with(|_| millimetres(2)).expect("core contour");
        for segment in core.segments() {
            for point in [segment.start(), segment.end()] {
                assert!(point.y >= 2.0 - 1.0e-6, "core point {point:?} lies under the bottom band");
                assert!(point.x <= 98.0 + 1.0e-6, "core point {point:?} lies over the right band");
            }
        }
        assert!(
            core.segments().iter().any(|segment| (segment.start() - DVec2::new(88.0, 2.0)).length() < 1.0e-6
                || (segment.end() - DVec2::new(88.0, 2.0)).length() < 1.0e-6),
            "the bottom band's inner face must meet the chamfer at (88, 2): {:?}",
            core.segments()
        );
    }

    #[test]
    fn outward_bow_extends_the_bounds_and_inward_bow_does_not() {
        let mut outward = panel(100, 60);
        outward.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(10)).expect("bow"));
        let (min, max) = outward.nominal_contour().expect("contour").bounds().expect("bounds");
        assert!((min.y + 10.0).abs() < 1e-9, "min {min:?}");
        assert!((max.y - 60.0).abs() < 1e-9, "max {max:?}");

        let mut inward = panel(100, 60);
        inward.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(-10)).expect("bow"));
        let (min, _) = inward.nominal_contour().expect("contour").bounds().expect("bounds");
        assert!(min.y.abs() < 1e-9, "min {min:?}");
    }

    #[test]
    fn corner_features_that_consume_an_edge_are_rejected() {
        let mut panel = panel(100, 60);
        for corner in [RectCorner::BottomLeft, RectCorner::BottomRight] {
            panel.corner_features.push(
                CornerFeature::new(corner.id(), CornerFeatureKind::Radius { radius: millimetres(50) }).expect("radius"),
            );
        }
        assert!(matches!(
            panel.nominal_contour(),
            Err(PanelContourError::FeatureOverlap { edge }) if edge == RectEdge::Bottom.id()
        ));
    }

    #[test]
    fn too_deep_bow_is_rejected() {
        let mut panel = panel(100, 60);
        panel.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(51)).expect("bow"));
        assert!(matches!(
            panel.nominal_contour(),
            Err(PanelContourError::BowTooDeep { edge }) if edge == RectEdge::Bottom.id()
        ));
    }

    #[test]
    fn a_rectangle_has_one_path_per_edge() {
        let panel = panel(100, 60);
        let paths = panel.contour_paths().expect("paths");
        assert_eq!(paths.len(), 4);
        let mut edges = Vec::new();
        for path in &paths {
            assert!(!path.is_closed());
            assert_eq!(path.sources().len(), 1);
            assert_eq!(path.lone_corner(), None);
            edges.extend(path.edge_ids());
        }
        edges.sort_by_key(|edge| edge.get());
        assert_eq!(edges, RectEdge::ALL.map(RectEdge::id).to_vec());
    }

    #[test]
    fn a_radius_joins_its_two_edges_into_one_path() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(10) })
                .expect("radius"),
        );
        let paths = panel.contour_paths().expect("paths");
        assert_eq!(paths.len(), 3);
        let joined = paths.iter().find(|path| path.sources().len() == 3).expect("path through the radius");
        assert_eq!(
            joined.sources(),
            &[
                SegmentSource::Edge(RectEdge::Right.id()),
                SegmentSource::Corner(RectCorner::TopRight.id()),
                SegmentSource::Edge(RectEdge::Top.id()),
            ]
        );
        assert_eq!(joined.edge_ids().collect::<Vec<_>>(), vec![RectEdge::Right.id(), RectEdge::Top.id()]);
    }

    #[test]
    fn banding_an_edge_keeps_the_rest_of_its_path_unbanded() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(RectCorner::TopRight.id(), CornerFeatureKind::Radius { radius: millimetres(10) })
                .expect("radius"),
        );
        let material = MaterialId::new(1).expect("material");
        let band =
            EdgeBanding::new(RectEdge::Right.id(), material, false, millimetres(2), millimetres(20)).expect("band");
        panel.apply_edge_banding(band).expect("apply band");
        let covered = panel.edge_banding().iter().map(EdgeBanding::edge).collect::<Vec<_>>();
        assert_eq!(covered, vec![RectEdge::Right.id()]);
        assert!(panel.edge_banding().iter().all(|band| band.material() == material));

        assert!(!panel.clear_edge_banding_path(RectEdge::Top.id()).expect("clear an unbanded edge"));
        assert_eq!(panel.edge_banding().len(), 1);
        assert!(panel.clear_edge_banding_path(RectEdge::Right.id()).expect("clear the banded edge"));
        assert!(panel.edge_banding().is_empty());
    }

    #[test]
    fn banding_outside_the_outline_is_rejected() {
        let mut panel = panel(100, 60);
        let edge = EdgeId::new(9).expect("edge id");
        let material = MaterialId::new(1).expect("material");
        let band = EdgeBanding::new(edge, material, false, millimetres(2), millimetres(20)).expect("band");
        assert_eq!(panel.apply_edge_banding(band), Err(PanelContourError::UnsupportedEdge { edge }));
    }

    #[test]
    fn a_chamfer_is_a_separate_path_with_a_butt_joint() {
        let mut panel = panel(100, 60);
        panel.corner_features.push(
            CornerFeature::new(
                RectCorner::TopRight.id(),
                CornerFeatureKind::Chamfer { incoming: millimetres(10), outgoing: millimetres(14) },
            )
            .expect("chamfer"),
        );
        let paths = panel.contour_paths().expect("paths");
        assert_eq!(paths.len(), 5);
        let chamfer =
            paths.iter().find(|path| path.lone_corner() == Some(RectCorner::TopRight.id())).expect("chamfer path");
        assert_eq!(chamfer.sources(), &[SegmentSource::Corner(RectCorner::TopRight.id())]);
        assert_eq!(chamfer.edge_ids().count(), 0);
    }

    #[test]
    fn a_fully_rounded_panel_is_one_closed_path() {
        let mut panel = panel(100, 60);
        for corner in RectCorner::ALL {
            panel.corner_features.push(
                CornerFeature::new(corner.id(), CornerFeatureKind::Radius { radius: millimetres(8) }).expect("radius"),
            );
        }
        let paths = panel.contour_paths().expect("paths");
        assert_eq!(paths.len(), 1);
        let path = paths.first().expect("path");
        assert!(path.is_closed());
        assert_eq!(path.sources().len(), 8);
        assert_eq!(path.edge_ids().count(), 4);
    }

    #[test]
    fn a_bowed_edge_stays_one_path() {
        let mut panel = panel(100, 60);
        panel.edge_bows.push(EdgeBow::new(RectEdge::Bottom.id(), millimetres(5)).expect("bow"));
        let paths = panel.contour_paths().expect("paths");
        assert_eq!(paths.len(), 4);
        let bottom = paths.iter().find(|path| path.edge_ids().eq([RectEdge::Bottom.id()])).expect("bottom path");
        assert_eq!(bottom.sources(), &[SegmentSource::Edge(RectEdge::Bottom.id())]);
    }
}
