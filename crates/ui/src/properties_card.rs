//! The selected node's property card.
//!
//! Every node — a panel or a container — carries typed properties. The card lists them, edits the
//! scalar kinds in place and adds a new property by key and kind. Every edit is an
//! `node.property.set` or `node.property.clear` command, so it is undoable like any other document
//! change. Reserved system keys are validated by the model, and a rejected value is shown as an
//! error instead of being coerced.
//!
//! A number is the shared field ([ADR 0092]): an integer, a length or an angle is the same 76 px box
//! with its unit outside it, and one drag or one typing run is one undo entry through the session
//! edit scope. A length or an angle keeps its number in a draft keyed by the property name, because
//! a row moves when another property is added or removed.
//!
//! [ADR 0092]: ../docs/decisions/0092-a-field-is-a-number-with-its-unit-outside.md

use std::collections::BTreeMap;

use egui::{RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::{NODE_PROPERTY_CLEAR_ID, NODE_PROPERTY_SET_ID};
use furnicraft_engine::{EditScope, NodePropertyClear, NodePropertySet, Session};
use furnicraft_model::{ClassCode, Id, PropertyValue};
use furnicraft_text::Message;
use furnicraft_units::{Degrees, Length};

use crate::command::{dispatch, json};
use crate::scrub::{self, CommitPolicy, Scrub};
use crate::section::section;
use crate::theme;

/// The property kinds the card can add without a prototype schema.
const NEW_KIND_NAMES: [Message; 6] = [
    Message::PropertyKindText,
    Message::PropertyKindInteger,
    Message::PropertyKindBool,
    Message::PropertyKindLength,
    Message::PropertyKindAngle,
    Message::PropertyKindClass,
];

fn new_value(kind: usize) -> PropertyValue {
    match kind {
        1 => PropertyValue::integer(0),
        2 => PropertyValue::bool(false),
        3 => PropertyValue::length(Length::ZERO),
        4 => PropertyValue::angle(Degrees::ZERO),
        5 => PropertyValue::class(ClassCode::UNCLASSIFIED),
        _ => PropertyValue::text(""),
    }
}

/// Persistent draft state of the property card.
#[derive(Debug, Default)]
pub(crate) struct PropertiesCard {
    node: Option<Id>,
    drafts: Drafts,
    new_key: String,
    new_kind: usize,
    error: Option<String>,
}

/// The drafts the rows keep between frames, keyed by property name.
///
/// A text row keeps the string the user is typing; a length or an angle keeps the number the field
/// owns. Both are keyed by name so that adding or removing a row moves no draft to another property.
#[derive(Debug, Default)]
struct Drafts {
    text: BTreeMap<String, String>,
    numbers: BTreeMap<String, NumberDraft>,
}

/// The number a length or an angle row's field holds, and the value it was read from.
///
/// `base` detects a change made elsewhere — by undo, or by another card — so the field re-reads the
/// document instead of keeping a number the property no longer has.
#[derive(Clone, Debug)]
struct NumberDraft {
    base: PropertyValue,
    value: f64,
}

impl NumberDraft {
    /// The draft for a value, in the field's own unit: millimetres or degrees.
    fn read(value: &PropertyValue) -> Self {
        let units = match value {
            PropertyValue::Length { value } => value.millimetres_f64(),
            PropertyValue::Angle { value } => value.degrees_f64(),
            _ => 0.0,
        };
        Self { base: value.clone(), value: units }
    }
}

/// One edit the card asks the editor to dispatch after drawing.
#[derive(Clone, Debug, PartialEq)]
enum Edit {
    /// Set a property; the scope folds a running field gesture into one undo entry.
    Set { key: String, value: PropertyValue, scope: Option<EditScope> },
    /// Clear a property.
    Clear { key: String },
}

/// What the rows of the card asked for this frame.
#[derive(Debug, Default)]
struct CardEdits {
    edits: Vec<Edit>,
    /// Scopes Escape abandoned, which the card restores.
    cancelled: Vec<EditScope>,
    /// Why the last typed text was refused, for the card's error line.
    refused: Option<Message>,
}

impl CardEdits {
    /// Folds one field's outward report in, and says whether it handed its number over.
    fn committed(&mut self, outcome: &scrub::FieldOutcome) -> bool {
        if let Some(scope) = outcome.cancelled {
            self.cancelled.push(scope);
        }
        if let Some(message) = outcome.error {
            self.refused = Some(message);
        }
        outcome.commit
    }
}

impl PropertiesCard {
    /// Draws the properties of one node and dispatches the accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, node: Id) {
        if node == session.project().root().id() {
            return;
        }
        if self.node != Some(node) {
            self.node = Some(node);
            self.drafts = Drafts::default();
            self.new_key.clear();
            self.error = None;
        }
        let Some(properties) = session.project().properties(node).cloned() else {
            return;
        };
        let is_component = session.project().is_component(node);
        // One token per frame for the number fields: the edit that takes it is one undo entry.
        let fresh = session.edit_scope();
        let mut changes = CardEdits::default();

        ui.separator();
        let lang = crate::language(ui.ctx());
        section(ui, "properties", Message::PropertiesTitle.text(lang), true, |ui| {
            ui.horizontal(|ui| {
                let status = if is_component { Message::ContainerRoleComponent } else { Message::ContainerRoleGroup };
                ui.label(RichText::new(status.text(lang)).strong());
                if let Some(class) = session.project().class(node) {
                    let text = if class.is_unclassified() {
                        Message::Unclassified.text(lang).to_owned()
                    } else {
                        Message::ClassWith.with(lang, [class])
                    };
                    ui.label(text);
                }
            });

            if properties.is_empty() {
                ui.label(Message::NoProperties.text(lang));
            }
            for (key, value) in properties.iter() {
                edit_row(ui, key, value, &mut self.drafts, fresh, &mut changes);
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.new_key).hint_text(Message::KeyHint.text(lang)).desired_width(140.0),
                );
                egui::ComboBox::from_id_salt("properties.new_kind")
                    .selected_text(
                        NEW_KIND_NAMES.get(self.new_kind).copied().unwrap_or(Message::PropertyKindText).text(lang),
                    )
                    .show_ui(ui, |ui| {
                        for (index, name) in NEW_KIND_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.new_kind, index, name.text(lang));
                        }
                    });
                let can_add = valid_new_key(&self.new_key) && !properties.contains_key(self.new_key.trim());
                if ui.add_enabled(can_add, egui::Button::new(Message::Add.text(lang))).clicked() {
                    let key = self.new_key.trim().to_owned();
                    changes.edits.push(Edit::Set { key: key.clone(), value: new_value(self.new_kind), scope: None });
                    self.drafts.text.remove(&key);
                    self.drafts.numbers.remove(&key);
                    self.new_key.clear();
                }
            });
        });

        for scope in changes.cancelled {
            session.cancel_edit(scope);
        }
        for edit in changes.edits {
            if let Err(error) = apply(session, node, edit) {
                self.error = Some(error);
                self.drafts.text.clear();
                break;
            }
            self.drafts.text.clear();
        }
        if let Some(message) = changes.refused {
            self.error = Some(message.text(crate::language(ui.ctx())).to_owned());
        }
        if let Some(error) = &self.error {
            ui.colored_label(theme::danger(ui), error);
        }
    }
}

fn valid_new_key(key: &str) -> bool {
    let key = key.trim();
    !key.is_empty() && !key.chars().any(char::is_whitespace)
}

fn apply(session: &mut Session, node: Id, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::Set { key, value, scope } => {
            let params = json(&NodePropertySet { node, key, value })?;
            match scope {
                Some(scope) => session
                    .dispatch_in(scope, NODE_PROPERTY_SET_ID, &params)
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                None => dispatch(session, NODE_PROPERTY_SET_ID, &params).map(|_| ()),
            }
        }
        Edit::Clear { key } => {
            let params = json(&NodePropertyClear { node, key })?;
            dispatch(session, NODE_PROPERTY_CLEAR_ID, &params).map(|_| ())
        }
    }
}

/// Draws one property row, pushing an edit when the value changes.
fn edit_row(
    ui: &mut Ui,
    key: &str,
    value: &PropertyValue,
    drafts: &mut Drafts,
    fresh: EditScope,
    changes: &mut CardEdits,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).strong());
        match value {
            PropertyValue::Bool { value } => {
                let mut checked = *value;
                if ui.checkbox(&mut checked, "").changed() {
                    changes.edits.push(Edit::Set {
                        key: key.to_owned(),
                        value: PropertyValue::bool(checked),
                        scope: None,
                    });
                }
            }
            PropertyValue::Integer { value } => {
                let mut number = *value;
                let outcome =
                    Scrub::new(&mut number).speed(1.0).decimals(0).policy(CommitPolicy::Live).edit(fresh).show(ui);
                if changes.committed(&outcome) {
                    changes.edits.push(Edit::Set {
                        key: key.to_owned(),
                        value: PropertyValue::integer(number),
                        scope: outcome.scope,
                    });
                }
            }
            PropertyValue::Length { .. } => {
                // A length keeps its millimetres in a draft keyed by the property name; `read`
                // re-reads the document when an edit elsewhere moves the property.
                let draft = drafts.numbers.entry(key.to_owned()).or_insert_with(|| NumberDraft::read(value));
                if draft.base != *value {
                    *draft = NumberDraft::read(value);
                }
                let outcome = Scrub::new(&mut draft.value)
                    .speed(0.5)
                    .range(-1_000_000.0..=1_000_000.0)
                    .decimals(3)
                    .unit("mm")
                    .parse(scrub::parse_millimetres)
                    .policy(CommitPolicy::Live)
                    .edit(fresh)
                    .show(ui);
                if changes.committed(&outcome) {
                    match scrub::millimetres(draft.value) {
                        Some(length) => changes.edits.push(Edit::Set {
                            key: key.to_owned(),
                            value: PropertyValue::length(length),
                            scope: outcome.scope,
                        }),
                        None => changes.refused = Some(Message::LengthErrorOutOfRange),
                    }
                }
            }
            PropertyValue::Angle { .. } => {
                let draft = drafts.numbers.entry(key.to_owned()).or_insert_with(|| NumberDraft::read(value));
                if draft.base != *value {
                    *draft = NumberDraft::read(value);
                }
                let outcome = Scrub::new(&mut draft.value)
                    .speed(1.0)
                    .range(-1_000_000.0..=1_000_000.0)
                    .decimals(3)
                    .unit("°")
                    .parse(scrub::parse_degrees)
                    .policy(CommitPolicy::Live)
                    .edit(fresh)
                    .show(ui);
                if changes.committed(&outcome) {
                    match scrub::degrees(draft.value) {
                        Some(angle) => changes.edits.push(Edit::Set {
                            key: key.to_owned(),
                            value: PropertyValue::angle(angle),
                            scope: outcome.scope,
                        }),
                        None => changes.refused = Some(Message::DegreesErrorOutOfRange),
                    }
                }
            }
            PropertyValue::Text { value } => {
                let draft = drafts.text.entry(key.to_owned()).or_insert_with(|| value.clone());
                let response = ui.add(TextEdit::singleline(draft).desired_width(180.0));
                if committed(&response) && draft.as_str() != value.as_str() {
                    let text = draft.clone();
                    drafts.text.remove(key);
                    changes.edits.push(Edit::Set {
                        key: key.to_owned(),
                        value: PropertyValue::text(&text),
                        scope: None,
                    });
                }
            }
            PropertyValue::Class { value } => {
                let draft = drafts.text.entry(key.to_owned()).or_insert_with(|| value.to_string());
                let response = ui.add(TextEdit::singleline(draft).hint_text("GG.TT.VV").desired_width(100.0));
                if committed(&response) {
                    match draft.parse::<ClassCode>() {
                        Ok(class) => {
                            drafts.text.remove(key);
                            changes.edits.push(Edit::Set {
                                key: key.to_owned(),
                                value: PropertyValue::class(class),
                                scope: None,
                            });
                        }
                        Err(_) => {
                            ui.colored_label(theme::danger(ui), "GG.TT.VV");
                        }
                    }
                }
            }
            PropertyValue::Choice { label, .. } => {
                ui.label(label);
            }
            PropertyValue::Reference { id } => {
                ui.label(format!("#{}", id.get()));
            }
            PropertyValue::Structured { value } => {
                ui.label(RichText::new(value.to_string()).weak());
            }
        }
        if ui.small_button("✕").on_hover_text(Message::CmdClearProperty.text(crate::language(ui.ctx()))).clicked() {
            changes.edits.push(Edit::Clear { key: key.to_owned() });
        }
    });
}

fn committed(response: &egui::Response) -> bool {
    response.lost_focus()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_text::Lang;
    use furnicraft_units::Degrees;

    use super::*;

    #[test]
    fn new_keys_must_be_non_empty_and_unspaced() {
        assert!(valid_new_key("finish"));
        assert!(valid_new_key("supplier.code"));
        assert!(!valid_new_key(""));
        assert!(!valid_new_key("   "));
        assert!(!valid_new_key("two words"));
    }

    #[test]
    fn applying_a_property_edit_dispatches_one_undoable_change() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply(
            &mut session,
            panel,
            Edit::Set { key: "finish".to_owned(), value: PropertyValue::text("oak"), scope: None },
        )
        .expect("set property");
        assert_eq!(
            session.project().properties(panel).and_then(|properties| properties.get("finish")),
            Some(&PropertyValue::text("oak"))
        );
        session.undo().expect("undo");
        assert!(!session.project().properties(panel).expect("properties").contains_key("finish"));
    }

    #[test]
    fn clearing_a_missing_property_is_an_error() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        assert!(apply(&mut session, panel, Edit::Clear { key: "missing".to_owned() }).is_err());
    }

    fn length_property(millimetres: i64) -> PropertyValue {
        PropertyValue::length(Length::from_millimetres(millimetres).expect("length"))
    }

    fn stored_length(session: &Session, panel: Id) -> Option<PropertyValue> {
        session.project().properties(panel).and_then(|properties| properties.get("thickness")).cloned()
    }

    #[test]
    fn a_number_draft_reads_the_field_unit() {
        let length = length_property(18);
        let draft = NumberDraft::read(&length);
        assert_eq!(draft.value, 18.0, "a length field works in millimetres");
        assert_eq!(draft.base, length, "and remembers what it read");

        let angle = PropertyValue::angle(Degrees::from_millidegrees(90_000));
        assert_eq!(NumberDraft::read(&angle).value, 90.0, "an angle field works in degrees");
    }

    #[test]
    fn a_dragged_length_is_one_undo_entry() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply(&mut session, panel, Edit::Set { key: "thickness".to_owned(), value: length_property(10), scope: None })
            .expect("set property");

        let scope = session.edit_scope();
        for millimetres in [12, 14, 16] {
            apply(
                &mut session,
                panel,
                Edit::Set { key: "thickness".to_owned(), value: length_property(millimetres), scope: Some(scope) },
            )
            .expect("the field writes while the hand moves");
        }
        session.end_edit();
        assert_eq!(stored_length(&session, panel), Some(length_property(16)));

        session.undo().expect("undo the drag");
        assert_eq!(stored_length(&session, panel), Some(length_property(10)), "one undo returns the whole drag");
    }

    fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        use egui::epaint::Shape;
        fn walk(shape: &Shape, out: &mut Vec<String>) {
            match shape {
                Shape::Text(text) => out.push(text.galley.text().to_owned()),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn a_length_is_drawn_as_a_number_with_the_unit_outside_it() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let panel = Id::new(2).expect("panel id");
        apply(&mut session, panel, Edit::Set { key: "thickness".to_owned(), value: length_property(18), scope: None })
            .expect("set property");

        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        crate::set_language(&ctx, Lang::En);
        let mut card = PropertiesCard::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            card.show(ui, &mut session, panel);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text == "18"), "the number is the field's own: {texts:?}");
        assert!(texts.iter().any(|text| text == "mm"), "the unit is drawn outside the box: {texts:?}");
        assert!(
            !texts.iter().any(|text| text.ends_with(" mm")),
            "the unit never enters the editable string: {texts:?}"
        );
    }
}
