//! The cutout section of the panel card.
//!
//! A cutout is the one operation kind this build knows: a region removed from a face, through the
//! panel or to a blind depth. The section edits the parameters the record stores and reports them as
//! one replacement, so a command validates the whole shape before the document changes.

use egui::{ComboBox, Ui};
use furnicraft_model::{Cutout, CutoutDepth, CutoutShape, Id, MachiningId, PanelRect};
use furnicraft_text::Message;
use furnicraft_units::Length;

use crate::contour_edit::ContourEntry;
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

    const fn label(self) -> Message {
        match self {
            Self::Through => Message::Through,
            Self::Blind => Message::Blind,
        }
    }

    const fn of(depth: CutoutDepth) -> Self {
        match depth {
            CutoutDepth::Through => Self::Through,
            CutoutDepth::Blind { .. } => Self::Blind,
        }
    }
}

/// What the cutout section wants after one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CutoutEdit {
    /// Replacement parameters when a field committed a change.
    pub(crate) params: Option<Cutout>,
    /// Whether the user asked to reshape the stored ring in the viewport.
    pub(crate) edit_contour: bool,
}

impl CutoutEdit {
    /// The changes this frame carries.
    const fn new(params: Option<Cutout>, edit_contour: bool) -> Self {
        Self { params, edit_contour }
    }
}

/// Draws the cutout body and reports edited parameters and a contour-edit request.
///
/// `entry` tells the section whether this record's ring is already being reshaped in the viewport,
/// whether another contour edit owns it, or whether the selection rule blocks the mode.
pub(crate) fn show(
    ui: &mut Ui,
    panel_id: Id,
    machining: MachiningId,
    cutout: &Cutout,
    thickness: Length,
    entry: ContourEntry,
) -> CutoutEdit {
    let (shape, edit_contour) = show_shape(ui, cutout.shape(), entry);
    let depth = show_depth(ui, panel_id, machining, cutout.depth(), thickness);
    let params = (shape.is_some() || depth.is_some()).then(|| {
        let shape = shape.unwrap_or_else(|| cutout.shape().clone());
        let depth = depth.unwrap_or(cutout.depth());
        Cutout::new(shape, depth)
    });
    CutoutEdit::new(params, edit_contour)
}

/// Draws the shape controls. A rectangle is edited in place; a stored ring is a summary with the
/// viewport edit entry, because it comes from the contour edit mode rather than from four numbers.
fn show_shape(ui: &mut Ui, shape: &CutoutShape, entry: ContourEntry) -> (Option<CutoutShape>, bool) {
    match shape {
        CutoutShape::Rectangle(rect) => (show_rectangle(ui, *rect).map(CutoutShape::Rectangle), false),
        CutoutShape::Contour(contour) => {
            let segments = contour.segments().len();
            let arcs = contour.segments().filter(|segment| segment.is_arc()).count();
            let lang = crate::language(ui.ctx());
            let mut summary = Message::counted(
                segments,
                Message::PanelContourRingOne,
                Message::PanelContourRingFew,
                Message::PanelContourRingMany,
            )
            .text(lang);
            if arcs > 0 {
                summary.push_str(
                    &Message::counted(arcs, Message::ArcSuffixOne, Message::ArcSuffixFew, Message::ArcSuffixMany)
                        .text(lang),
                );
            }
            let mut edit_contour = false;
            ui.horizontal(|ui| {
                ui.label(summary);
                match entry {
                    ContourEntry::Editing => {
                        ui.label(Message::PanelEditingInViewport.text(lang));
                    }
                    ContourEntry::Busy => {
                        ui.label(Message::PanelFinishContour.text(lang));
                    }
                    ContourEntry::SelectSingle => {
                        ui.label(Message::PanelSelectSingle.text(lang));
                    }
                    ContourEntry::Ready => {
                        edit_contour = ui
                            .button(Message::PanelEditContour.text(lang))
                            .on_hover_text(Message::CutoutReshapeHint.text(lang))
                            .clicked();
                    }
                }
            });
            (None, edit_contour)
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
        ui.label(Message::Width.text(crate::language(ui.ctx())));
        let (width, _) = millimetre_field(ui, rect.width(), 0.1..=10_000.0);
        if let Some(width) = width.and_then(positive_length) {
            next = PanelRect::new(next.x(), next.y(), width, next.height()).unwrap_or(next);
            committed = true;
        }
        ui.label(Message::FieldHeight.text(crate::language(ui.ctx())));
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
        ui.label(Message::Depth.text(crate::language(ui.ctx())));
        ComboBox::from_id_salt(("cutout_depth", panel_id.get(), machining.get()))
            .selected_text(current.label().text(crate::language(ui.ctx())))
            .show_ui(ui, |ui| {
                for option in DepthMode::ALL {
                    if ui.selectable_label(mode == option, option.label().text(crate::language(ui.ctx()))).clicked()
                        && mode != option
                    {
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
