//! The origin card shared by every node kind.
//!
//! The coordinates are the origin local to the parent container and are entered in millimetres; a
//! read-only *World origin* row shows what the parent chain composes to, read from
//! `Project::origin_world`.
//! Rotation is shown as intrinsic XYZ Euler angles in degrees; the document keeps a unit
//! quaternion, so the angles are only a view of it. Every field is a scrub: a drag writes the
//! document while the hand moves and the whole gesture is one undo entry, typed text lands after a
//! short debounce, and Escape during a drag puts the value back. Each field changes its own
//! component and leaves the other five exactly as the document holds them, so a translation edit
//! keeps the stored quaternion bit-for-bit and a rotation edit keeps the stored translation.
//! *Place at world zero* dispatches `node.placement.world_zero`, which clears the parent-frame
//! translation and keeps the rotation.

use std::ops::RangeInclusive;

use egui::{RichText, Ui};
use furnicraft_engine::node_cmds::{NODE_PLACEMENT_SET_ID, NODE_PLACEMENT_WORLD_ZERO_ID};
use furnicraft_engine::{EditScope, NodePlacementSet, NodePlacementWorldZero, Session};
use furnicraft_model::{EulerAngles, Id, Placement};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::coords;
use crate::scrub::{self, CommitPolicy, Parse, Scrub};
use crate::section::section;
use crate::theme;

/// Persistent state of the placement card.
#[derive(Debug, Default)]
pub(crate) struct PlacementCard {
    draft: Option<Draft>,
    error: Option<String>,
}

/// The placement as the fields show it, and the document placement it was read from.
///
/// `base` detects a change made elsewhere, for example by undo or the viewport: the card re-reads
/// the document instead of letting a stale draft overwrite a restored placement. The numbers are
/// millimetres and degrees because that is what a field scrubs; the document keeps micrometres and a
/// unit quaternion, and the card converts at the command boundary.
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    node: Id,
    base: Placement,
    x: f64,
    y: f64,
    z: f64,
    rx: f64,
    ry: f64,
    rz: f64,
}

impl Draft {
    fn new(node: Id, base: Placement) -> Option<Self> {
        let [x, y, z] = base.translation_lengths()?;
        let rotation = base.rotation_euler();
        Some(Self {
            node,
            base,
            x: x.millimetres_f64(),
            y: y.millimetres_f64(),
            z: z.millimetres_f64(),
            rx: rotation.x.degrees_f64(),
            ry: rotation.y.degrees_f64(),
            rz: rotation.z.degrees_f64(),
        })
    }

    /// The component one field edits.
    fn axis(&mut self, axis: Axis) -> &mut f64 {
        match axis {
            Axis::X => &mut self.x,
            Axis::Y => &mut self.y,
            Axis::Z => &mut self.z,
            Axis::Rx => &mut self.rx,
            Axis::Ry => &mut self.ry,
            Axis::Rz => &mut self.rz,
        }
    }
}

/// One component of the placement: a translation axis in millimetres or a rotation axis in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
    Rx,
    Ry,
    Rz,
}

impl Axis {
    /// The three translation fields, in the order the card draws them.
    const TRANSLATION: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// The three rotation fields, in the order the card draws them.
    const ROTATION: [Self; 3] = [Self::Rx, Self::Ry, Self::Rz];

    fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Rx => "X",
            Self::Ry => "Y",
            Self::Rz => "Z",
        }
    }

    /// The unit drawn beside the field, never inside it.
    fn unit(self) -> &'static str {
        match self {
            Self::X | Self::Y | Self::Z => "mm",
            Self::Rx | Self::Ry | Self::Rz => "°",
        }
    }

    /// How the typed text becomes a number: a length or an angle, both with the document's precision.
    fn parse(self) -> Parse {
        match self {
            Self::X | Self::Y | Self::Z => scrub::parse_millimetres,
            Self::Rx | Self::Ry | Self::Rz => scrub::parse_degrees,
        }
    }

    /// The range a drag stays inside; the command validates what the field allows.
    fn range(self) -> RangeInclusive<f64> {
        match self {
            Self::X | Self::Y | Self::Z => -1_000_000.0..=1_000_000.0,
            Self::Rx | Self::Ry | Self::Rz => -360_000.0..=360_000.0,
        }
    }

    /// The index of a translation axis inside a three-component vector.
    fn index(self) -> Option<usize> {
        match self {
            Self::X => Some(0),
            Self::Y => Some(1),
            Self::Z => Some(2),
            Self::Rx | Self::Ry | Self::Rz => None,
        }
    }
}

impl PlacementCard {
    /// Draws the local placement of `node` and dispatches an accepted edit.
    ///
    /// The root container defines the world frame and has no placement, so it never reaches here.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, node: Id) {
        // One token per frame for the six fields: the edit that takes it owns one undo entry.
        let fresh = session.edit_scope();
        let Some(current) = session.project().node(node).map(|node| node.placement()) else {
            self.draft = None;
            self.error = None;
            return;
        };
        if self.draft.as_ref().is_none_or(|draft| draft.node != node || draft.base != current) {
            self.error = None;
            self.draft = Draft::new(node, current);
        }

        ui.label(RichText::new(Message::Origin.text(crate::language(ui.ctx()))).strong());
        let parent_name = session
            .project()
            .parent_of(node)
            .and_then(|parent| session.project().container(parent))
            .map(|container| container.name().to_owned());
        let lang = crate::language(ui.ctx());
        let origin_title = match parent_name {
            Some(name) => Message::OriginLocalTo.with(lang, [name]),
            None => Message::OriginLocalToParent.text(lang).to_owned(),
        };

        let Some(draft) = self.draft.as_mut() else {
            ui.colored_label(theme::danger(ui), Message::OriginOutOfRange.text(crate::language(ui.ctx())));
            return;
        };

        let world = coords::world_origin(session.project(), node);
        let can_zero = draft.base.translation_lengths().is_some_and(|translation| translation != [Length::ZERO; 3]);
        let mut zero = false;
        let mut edits: Vec<(Axis, f64, EditScope)> = Vec::new();
        let mut cancelled: Vec<EditScope> = Vec::new();
        let mut refused: Option<Message> = None;

        section(ui, "placement.transform", Message::SectionTransform.text(lang), true, |ui| {
            section(ui, "placement.transform.origin", origin_title.clone(), true, |ui| {
                for axis in Axis::TRANSLATION {
                    ui.horizontal(|ui| {
                        ui.label(axis.label());
                        let outcome = draw_axis(ui, axis, draft.axis(axis), fresh);
                        if outcome.commit
                            && let Some(scope) = outcome.scope
                        {
                            edits.push((axis, *draft.axis(axis), scope));
                        }
                        refused = refused.or(outcome.error);
                        cancelled.extend(outcome.cancelled);
                    });
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_zero, egui::Button::new(Message::CmdPlaceAtWorldZero.text(lang))).clicked() {
                        zero = true;
                    }
                });
                match &world {
                    Some(world) => ui.label(Message::WorldOrigin.with(lang, [world])),
                    None => ui.label(Message::WorldOriginOutOfRange.text(lang)),
                };
            });
            section(
                ui,
                "placement.transform.rotation",
                Message::RotationWithOrder.with(lang, [EulerAngles::ORDER_LABEL]),
                true,
                |ui| {
                    for axis in Axis::ROTATION {
                        ui.horizontal(|ui| {
                            ui.label(axis.label());
                            let outcome = draw_axis(ui, axis, draft.axis(axis), fresh);
                            if outcome.commit
                                && let Some(scope) = outcome.scope
                            {
                                edits.push((axis, *draft.axis(axis), scope));
                            }
                            refused = refused.or(outcome.error);
                            cancelled.extend(outcome.cancelled);
                        });
                    }
                },
            );
        });
        if let Some(message) = refused {
            ui.colored_label(theme::danger(ui), message.text(lang));
        }

        for scope in cancelled {
            session.cancel_edit(scope);
        }

        if zero {
            match apply_world_zero(session, node, lang) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
            return;
        }

        if let Some(error) = &self.error {
            ui.colored_label(theme::danger(ui), error);
        }
        if edits.is_empty() {
            return;
        }
        for (axis, value, scope) in edits {
            match apply_axis(session, node, axis, value, scope, lang) {
                Ok(()) => self.error = None,
                Err(error) => self.error = Some(error),
            }
        }
        self.draft = None;
        ui.ctx().request_repaint();
    }
}

/// Draws one axis field and hands back what it reported.
fn draw_axis(ui: &mut Ui, axis: Axis, value: &mut f64, fresh: EditScope) -> crate::scrub::FieldOutcome {
    Scrub::new(value)
        .speed(0.5)
        .range(axis.range())
        .decimals(3)
        .unit(axis.unit())
        .parse(axis.parse())
        .policy(CommitPolicy::Live)
        .edit(fresh)
        .show(ui)
}

/// Writes one component and leaves the rest of the placement exactly as the document holds it.
fn apply_axis(
    session: &mut Session,
    node: Id,
    axis: Axis,
    value: f64,
    scope: EditScope,
    lang: Lang,
) -> Result<(), String> {
    let current = session
        .project()
        .node(node)
        .map(|node| node.placement())
        .ok_or_else(|| Message::NodeNotInScene.with(lang, [node]))?;
    let mut placement = current;
    if let Some(index) = axis.index() {
        let mut translation = current.translation_lengths().ok_or_else(|| {
            Message::FieldWithError.with(lang, [axis.label(), Message::LengthErrorOutOfRange.text(lang)])
        })?;
        let Some(length) = scrub::millimetres(value) else {
            return Err(Message::FieldWithError.with(lang, [axis.label(), Message::LengthErrorOutOfRange.text(lang)]));
        };
        if let Some(slot) = translation.get_mut(index) {
            *slot = length;
        }
        if current.translation_lengths() != Some(translation) {
            placement = placement
                .with_translation(translation[0], translation[1], translation[2])
                .map_err(|error| error.to_string())?;
        }
    } else {
        let Some(angle) = scrub::degrees(value) else {
            return Err(Message::FieldWithError.with(lang, [axis.label(), Message::DegreesErrorOutOfRange.text(lang)]));
        };
        let mut rotation = current.rotation_euler();
        match axis {
            Axis::Rx => rotation.x = angle,
            Axis::Ry => rotation.y = angle,
            _ => rotation.z = angle,
        }
        if rotation.wrapped() != current.rotation_euler() {
            placement = placement.with_rotation_euler(rotation).map_err(|error| error.to_string())?;
        }
    }
    if placement.same_transform(current) {
        return Ok(());
    }
    let params = json(&NodePlacementSet { node, placement })?;
    session.dispatch_in(scope, NODE_PLACEMENT_SET_ID, &params).map(|_| ()).map_err(|error| error.to_string())
}

/// Clears the parent-frame translation through `node.placement.world_zero`, keeping the rotation.
fn apply_world_zero(session: &mut Session, node: Id, lang: Lang) -> Result<(), String> {
    if session.project().node(node).is_none() {
        return Err(Message::NodeNotInScene.with(lang, [node]));
    }
    let params = json(&NodePlacementWorldZero { node })?;
    dispatch(session, NODE_PLACEMENT_WORLD_ZERO_ID, &params).map(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_units::Degrees;

    use super::*;

    fn set_placement(session: &mut Session, node: Id, placement: Placement) {
        let params = json(&NodePlacementSet { node, placement }).expect("command parameters");
        dispatch(session, NODE_PLACEMENT_SET_ID, &params).expect("set placement");
    }

    /// Places `node` one axis at a time inside one edit, as a drag does.
    fn drag(session: &mut Session, node: Id, axis: Axis, values: &[f64]) {
        let scope = session.edit_scope();
        for value in values {
            apply_axis(session, node, axis, *value, scope, Lang::En).expect("axis edit");
        }
        session.end_edit();
    }

    #[test]
    fn an_axis_edit_moves_the_node_as_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        drag(&mut session, panel, Axis::X, &[10.0]);
        let placement = session.project().panel(panel).expect("panel").placement();
        assert_eq!(
            placement.translation_lengths(),
            Some([Length::from_micrometres(10_000), Length::ZERO, Length::ZERO])
        );
        assert_eq!(placement.rotation(), Placement::IDENTITY.rotation());

        session.undo().expect("undo the move");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert!(!session.can_undo(), "a move must be exactly one undo entry");
    }

    #[test]
    fn a_drag_that_writes_every_frame_is_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        drag(&mut session, panel, Axis::Y, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            session.project().panel(panel).expect("panel").placement().translation_lengths(),
            Some([Length::ZERO, Length::from_micrometres(4_000), Length::ZERO])
        );
        session.undo().expect("undo the whole drag");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert!(!session.can_undo(), "a drag is one undo entry, whatever number of frames it took");
    }

    #[test]
    fn a_rotation_edit_is_one_undo_step_and_keeps_the_translation() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        drag(&mut session, panel, Axis::Rx, &[90.0]);
        drag(&mut session, panel, Axis::Rz, &[45.0]);
        let placement = session.project().panel(panel).expect("panel").placement();
        assert_eq!(placement.translation(), Placement::IDENTITY.translation());
        let angles = placement.rotation_euler();
        assert_eq!(angles.x, Degrees::from_millidegrees(90_000));
        assert_eq!(angles.y, Degrees::ZERO);
        assert_eq!(angles.z, Degrees::from_millidegrees(45_000));

        session.undo().expect("undo the second turn");
        session.undo().expect("undo the first turn");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert!(!session.can_undo());
    }

    #[test]
    fn an_unchanged_value_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        drag(&mut session, panel, Axis::X, &[0.0]);
        drag(&mut session, panel, Axis::Rx, &[0.0]);
        assert!(!session.can_undo());
    }

    #[test]
    fn a_full_turn_or_a_wrapped_angle_does_not_touch_history() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");

        drag(&mut session, panel, Axis::Rx, &[360.0]);
        drag(&mut session, panel, Axis::Ry, &[-720.0]);
        assert!(!session.can_undo(), "a full turn is not a change");
    }

    #[test]
    fn a_hostile_value_is_refused_without_touching_the_document() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        let scope = session.edit_scope();

        let error =
            apply_axis(&mut session, panel, Axis::X, f64::INFINITY, scope, Lang::En).expect_err("an infinite length");
        assert!(error.starts_with("X: "), "error was {error}");
        let error = apply_axis(&mut session, panel, Axis::Rx, f64::NAN, scope, Lang::En).expect_err("a NaN angle");
        assert!(error.starts_with("X: "), "error was {error}");
        assert!(!session.can_undo());

        let missing = Id::new(999).expect("id");
        let error = apply_axis(&mut session, missing, Axis::X, 0.0, scope, Lang::En).expect_err("missing node");
        assert!(error.contains("not in the scene"), "error was {error}");
    }

    #[test]
    fn a_rotation_edit_keeps_the_exact_translation() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        let placement: Placement = serde_json::from_value(serde_json::json!({
            "translation": [1.234567, -0.0004, 5.5],
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }))
        .expect("placement");
        set_placement(&mut session, panel, placement);

        drag(&mut session, panel, Axis::Rx, &[45.0]);

        let after = session.project().panel(panel).expect("panel").placement();
        assert_eq!(after.translation(), placement.translation());
        assert_eq!(after.rotation_euler().x, Degrees::from_millidegrees(45_000));
    }

    #[test]
    fn a_translation_edit_keeps_the_exact_quaternion() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        let angle: f64 = 0.123_456;
        let placement: Placement = serde_json::from_value(serde_json::json!({
            "translation": [0.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, (angle / 2.0).sin(), (angle / 2.0).cos()]
        }))
        .expect("placement");
        set_placement(&mut session, panel, placement);

        drag(&mut session, panel, Axis::X, &[10.0]);

        let after = session.project().panel(panel).expect("panel").placement();
        assert_eq!(after.rotation(), placement.rotation());
        assert_eq!(after.translation_lengths(), Some([Length::from_micrometres(10_000), Length::ZERO, Length::ZERO]));
    }

    #[test]
    fn zeroing_clears_the_translation_and_keeps_the_rotation_as_one_undo_step() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let half = std::f64::consts::FRAC_PI_4;
        let start: Placement = serde_json::from_value(serde_json::json!({
            "translation": [10.0, -2.5, 3.0],
            "rotation": [0.0, 0.0, half.sin(), half.cos()]
        }))
        .expect("turned placement");
        project.set_node_placement(panel, start).expect("place the panel");
        let mut session = Session::with_catalog(project, sample_catalog());

        apply_world_zero(&mut session, panel, Lang::En).expect("zero the translation");

        let after = session.project().panel(panel).expect("panel").placement();
        assert_eq!(after.translation_lengths(), Some([Length::ZERO; 3]));
        assert_eq!(after.rotation(), start.rotation());

        session.undo().expect("undo world zero");
        assert_eq!(session.project().panel(panel).expect("panel").placement(), start);
        assert!(!session.can_undo(), "world zero must be exactly one undo entry");

        let error = apply_world_zero(&mut session, Id::new(999).expect("id"), Lang::En).expect_err("missing node");
        assert!(error.contains("not in the scene"), "error was {error}");
    }

    #[test]
    fn the_draft_prefills_from_the_document_without_units() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let placement: Placement = serde_json::from_value(serde_json::json!({
            "translation": [12.5, -7.0, 0.125],
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }))
        .expect("placement");
        project.set_node_placement(panel, placement).expect("place the panel");

        let draft = Draft::new(panel, placement).expect("representable translation");
        assert_eq!((draft.x, draft.y, draft.z), (12.5, -7.0, 0.125));
        assert_eq!((draft.rx, draft.ry, draft.rz), (0.0, 0.0, 0.0));
        assert_eq!(draft.base, placement);
    }

    #[test]
    fn a_hostile_translation_has_no_draft() {
        let huge: Placement = serde_json::from_value(serde_json::json!({
            "translation": [1e300, 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0, 1.0]
        }))
        .expect("finite placement");
        let node = Id::new(2).expect("id");
        assert!(Draft::new(node, huge).is_none());
    }
}
