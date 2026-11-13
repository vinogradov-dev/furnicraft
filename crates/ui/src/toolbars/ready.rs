//! What one ribbon cell draws and runs: the one place where a map row meets the session.
//!
//! The map says *where* a tool sits and what it is called; this module says whether it can run
//! *here* — a catalogue material, a closed path, a running draft, a viewport that owns its
//! projection — and returns the sentence that says what is missing, so a disabled cell still
//! explains itself. It also holds the one rule the button and the draft agree on: when a running
//! chain can be closed.

use furnicraft_model::Id;
use furnicraft_text::{Lang, Message};

use super::spec::{ToolKind, ToolSpec};
use super::state::Toolbars;
use super::{RibbonContext, ToolAction, is_armed, shortcut_label, strip_label};
use crate::draw::DrawTool;
use crate::icons::{self, Icon, tool_hint};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ready {
    /// The tool can run here.
    pub(super) enabled: bool,
    /// The button draws pressed: the tool owns the next click, or the toggle is on.
    pub(super) armed: bool,
    /// The tooltip and the accessible name, which is also the sentence of a refusal.
    pub(super) hint: String,
    /// The action a click asks for, unless the tool cannot run.
    pub(super) action: Option<ToolAction>,
}

/// Decides one tool against the picture the ribbon was handed.
///
/// This is the one place where a row's [`ToolKind`] meets the session: everything the map says about
/// *where* a tool sits is data, and everything about *whether* it can run is here, next to the words
/// that say why not.
pub(super) fn ready(kind: ToolKind, tool: &ToolSpec, state: &Toolbars, context: RibbonContext, lang: Lang) -> Ready {
    let RibbonContext { has_panel_material, selected, selected_path, active, owns_view, grid, draft, .. } = context;
    match kind {
        ToolKind::Draw(draw) => Ready {
            enabled: true,
            armed: is_armed(active, kind),
            hint: tool_hint(strip_label(draw).text(lang), shortcut_label(draw)),
            action: Some(ToolAction::Draw { tool: draw, sides: state.polygon_sides() }),
        },
        ToolKind::CloseDraft => {
            // Close needs a running draft that can close, cancel needs any running draft; both are
            // drawn so the pair never vanishes between clicks and the hint says why one is off.
            let (drawing, points, closed) = draft.unwrap_or((DrawTool::Line, 0, false));
            let close = draft_can_close(drawing, points, closed);
            let hint = if close { Message::DraftCloseHint.text(lang) } else { Message::DraftCloseNeedsTwo.text(lang) };
            Ready {
                enabled: close,
                armed: false,
                hint: hint.to_owned(),
                action: close.then_some(ToolAction::CloseDrawing),
            }
        }
        ToolKind::CancelDraft => {
            let running = draft.is_some();
            let hint = tool_hint(
                if running { Message::DraftCancelHint.text(lang) } else { Message::DraftNotRunning.text(lang) },
                Some("Esc"),
            );
            Ready { enabled: running, armed: false, hint, action: running.then_some(ToolAction::CancelDrawing) }
        }
        ToolKind::Primitive(primitive) => {
            let armed = is_armed(active, ToolKind::Primitive(primitive));
            let hint = Message::PrimitiveClickPlane.with(lang, [tool.label.text(lang)]);
            let action = ToolAction::CreatePrimitive { kind: primitive, size: state.primitive_size() };
            Ready { enabled: true, armed, hint, action: Some(action) }
        }
        ToolKind::Extrude => {
            // Only a closed path encloses a ring, so the button is off for anything else and the
            // hint names what is missing: a selection, a path, or a closed chain. The height is a
            // typed default, so a valid number is always there.
            let action = match (selected, selected_path) {
                (Some(object), Some(true)) => Some(ToolAction::ExtrudePath { object, height: state.extrude_height() }),
                _ => None,
            };
            Ready {
                enabled: action.is_some(),
                armed: false,
                hint: extrude_hint(selected, selected_path).text(lang).to_owned(),
                action,
            }
        }
        ToolKind::Product => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateProductCard),
        },
        ToolKind::Panel => Ready {
            enabled: has_panel_material,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreatePanel),
        },
        ToolKind::Assembly => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateAssembly),
        },
        ToolKind::Camera => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateCamera),
        },
        ToolKind::Light => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateLight),
        },
        ToolKind::FrameAll => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::FrameAll),
        },
        ToolKind::Projection => Ready {
            enabled: owns_view,
            armed: false,
            hint: tool_hint(Message::SwitchProjection.text(lang), tool.shortcut),
            action: owns_view.then_some(ToolAction::ToggleProjection),
        },
        ToolKind::GridSnap => Ready {
            enabled: owns_view,
            armed: grid,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: owns_view.then_some(ToolAction::ToggleGrid),
        },
    }
}

/// Whether a running draft can be closed into a ring: only a tool that closes, and only once the
/// chain has the two segments a ring needs, and only while it is still open.
///
/// Two segments are the whole requirement: a straight close adds the third, and a pending arc *is*
/// the closing segment (ADR 0077). This is the rule `DrawDraft::can_close` applies, so the button
/// and the first-node highlight never disagree with the command that runs.
pub const fn draft_can_close(tool: DrawTool, points: usize, closed: bool) -> bool {
    tool.closes() && points >= 2 && !closed
}

/// The icon of the projection toggle: the mode the view is in.
pub(super) fn projection_icon(perspective: bool) -> Icon {
    if perspective { icons::view::PERSPECTIVE } else { icons::view::ORTHOGRAPHIC }
}

/// The hint of the extrude button: what the tool does, or why it is off.
///
/// `selected_path` is `None` when the selection is not a path object, so a path that is merely
/// open gets its own message instead of being called "not a path".
pub(super) fn extrude_hint(selected: Option<Id>, selected_path: Option<bool>) -> Message {
    match (selected, selected_path) {
        (None, _) => Message::ExtrudeSelectPath,
        (Some(_), Some(true)) => Message::ExtrudeSelectedPath,
        (Some(_), Some(false)) => Message::ExtrudePathOpen,
        (Some(_), None) => Message::ExtrudeNotPath,
    }
}
