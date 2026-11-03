//! The cutout section of the panel card.
//!
//! A cutout is the one operation kind this build knows: a region removed from a face, through the
//! panel or to a blind depth. The section edits the parameters the record stores and reports them as
//! one replacement, so a command validates the whole shape before the document changes.

use egui::{ComboBox, Ui};
use furnicraft_model::{Cutout, CutoutDepth, CutoutShape, Id, MachiningId, PanelRect};
use furnicraft_units::Length;

use crate::panel_card::{millimetre_field, positive_length, signed_length};

/// The starting depth of a new blind cutout: half the panel, so it is legal on any usable panel.
fn default_blind_depth(thickness: Length) -> Length {
    let half = Length::from_micrometres(thickness.micrometres() / 2);
    if half.is_positive() { half } else { Length::from_millimetres(1).unwrap_or(Length::ZERO) }
}

/// The two depth modes the cutout editor offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DepthMode {
    Through,
    Blind,
}

impl DepthMode {
    const ALL: [Self; 2] = [Self::Through, Self::Blind];

    const fn label(self) -> &'static str {
        match self {
            Self::Through => "Through",
            Self::Blind => "Blind",
        }
    }

    const fn of(depth: CutoutDepth) -> Self {
        match depth {
            CutoutDepth::Through => Self::Through,
            CutoutDepth::Blind { .. } => Self::Blind,
        }
    }
}

/// Draws the cutout body and returns edited parameters when a field commits a change.
pub(crate) fn show(
    ui: &mut Ui,
    panel_id: Id,
    machining: MachiningId,
    cutout: &Cutout,
    thickness: Length,
) -> Option<Cutout> {
    let shape = show_shape(ui, cutout.shape());
    let depth = show_depth(ui, panel_id, machining, cutout.depth(), thickness);
    if shape.is_none() && depth.is_none() {
        return None;
    }
    let shape = shape.unwrap_or_else(|| cutout.shape().clone());
    let depth = depth.unwrap_or(cutout.depth());
    Some(Cutout::new(shape, depth))
}

/// Draws the shape controls. A rectangle is edited in place; a stored ring is a summary, because it
/// comes from the contour edit mode rather than from four numbers.
fn show_shape(ui: &mut Ui, shape: &CutoutShape) -> Option<CutoutShape> {
    match shape {
        CutoutShape::Rectangle(rect) => show_rectangle(ui, *rect).map(CutoutShape::Rectangle),
        CutoutShape::Contour(contour) => {
            let segments = contour.segments().len();
            let arcs = contour.segments().filter(|segment| segment.is_arc()).count();
            let suffix = if arcs > 0 { format!(" · {arcs} arcs") } else { String::new() };
            ui.label(format!("Contour ring · {segments} segments{suffix}"));
            None
        }
    }
}

/// Draws the four rectangle fields and returns a changed rectangle.
///
/// The fields keep the typed text until the edit ends and clamp it to the panel limit, so a commit
/// is always a usable rectangle; the command still decides whether it fits the core.
fn show_rectangle(ui: &mut Ui, rect: PanelRect) -> Option<PanelRect> {
    let mut next = rect;
    let mut committed = false;
    ui.horizontal(|ui| {
        ui.label("X");
        let (x, _) = millimetre_field(ui, rect.x(), -10_000.0..=10_000.0);
        if let Some(x) = x.and_then(signed_length) {
            next = PanelRect::new(x, next.y(), next.width(), next.height()).unwrap_or(next);
            committed = true;
        }
        ui.label("Y");
        let (y, _) = millimetre_field(ui, rect.y(), -10_000.0..=10_000.0);
        if let Some(y) = y.and_then(signed_length) {
            next = PanelRect::new(next.x(), y, next.width(), next.height()).unwrap_or(next);
            committed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Width");
        let (width, _) = millimetre_field(ui, rect.width(), 0.1..=10_000.0);
        if let Some(width) = width.and_then(positive_length) {
            next = PanelRect::new(next.x(), next.y(), width, next.height()).unwrap_or(next);
            committed = true;
        }
        ui.label("Height");
        let (height, _) = millimetre_field(ui, rect.height(), 0.1..=10_000.0);
        if let Some(height) = height.and_then(positive_length) {
            next = PanelRect::new(next.x(), next.y(), next.width(), height).unwrap_or(next);
            committed = true;
        }
    });
    (committed && next != rect).then_some(next)
}

/// Draws the depth mode and, for a blind cutout, its depth.
fn show_depth(
    ui: &mut Ui,
    panel_id: Id,
    machining: MachiningId,
    depth: CutoutDepth,
    thickness: Length,
) -> Option<CutoutDepth> {
    let current = DepthMode::of(depth);
    let mut mode = current;
    let mut next_depth = match depth {
        CutoutDepth::Through => None,
        CutoutDepth::Blind { depth } => Some(depth),
    };
    let mut committed = false;
    ui.horizontal(|ui| {
        ui.label("Depth");
        ComboBox::from_id_salt(("cutout_depth", panel_id.get(), machining.get()))
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for option in DepthMode::ALL {
                    if ui.selectable_label(mode == option, option.label()).clicked() && mode != option {
                        mode = option;
                        committed = true;
                    }
                }
            });
        if mode == DepthMode::Blind {
            let depth = next_depth.unwrap_or_else(|| default_blind_depth(thickness));
            let (value, _) = millimetre_field(ui, depth, 0.1..=10_000.0);
            if let Some(value) = value.and_then(positive_length) {
                next_depth = Some(value);
                committed = true;
            }
        }
    });
    if !committed {
        return None;
    }
    let next = match mode {
        DepthMode::Through => CutoutDepth::Through,
        DepthMode::Blind => CutoutDepth::Blind { depth: next_depth.unwrap_or_else(|| default_blind_depth(thickness)) },
    };
    (next != depth).then_some(next)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn mm(millimetres: i64) -> Length {
        Length::from_millimetres(millimetres).expect("millimetres")
    }

    #[test]
    fn a_new_blind_cutout_starts_at_half_the_panel() {
        assert_eq!(default_blind_depth(mm(18)), mm(9));
        assert_eq!(default_blind_depth(Length::ZERO), mm(1));
    }

    #[test]
    fn depth_mode_reads_the_stored_depth() {
        assert_eq!(DepthMode::of(CutoutDepth::Through), DepthMode::Through);
        assert_eq!(DepthMode::of(CutoutDepth::Blind { depth: mm(8) }), DepthMode::Blind);
    }
}
