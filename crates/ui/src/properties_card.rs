//! The selected node's property card.
//!
//! Every node — a panel or a container — carries typed properties. The card lists them, edits the
//! scalar kinds in place and adds a new property by key and kind. Every edit is an
//! `node.property.set` or `node.property.clear` command, so it is undoable like any other document
//! change. Reserved system keys are validated by the model, and a rejected value is shown as an
//! error instead of being coerced.

use std::collections::BTreeMap;

use egui::{Color32, DragValue, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::{NODE_PROPERTY_CLEAR_ID, NODE_PROPERTY_SET_ID};
use furnicraft_engine::{NodePropertyClear, NodePropertySet, Session};
use furnicraft_model::{FurnType, Id, PropertyValue};
use furnicraft_units::{Degrees, Length};

use crate::command::{dispatch, json};
use crate::section::section;

/// The property kinds the card can add without a prototype schema.
const NEW_KIND_NAMES: [&str; 6] = ["Text", "Integer", "Bool", "Length", "Angle", "Furniture type"];

fn new_value(kind: usize) -> PropertyValue {
    match kind {
        1 => PropertyValue::integer(0),
        2 => PropertyValue::bool(false),
        3 => PropertyValue::length(Length::ZERO),
        4 => PropertyValue::angle(Degrees::ZERO),
        5 => PropertyValue::furn_type(FurnType::UNCLASSIFIED),
        _ => PropertyValue::text(""),
    }
}

/// Persistent draft state of the property card.
#[derive(Debug, Default)]
pub(crate) struct PropertiesCard {
    node: Option<Id>,
    drafts: BTreeMap<String, String>,
    new_key: String,
    new_kind: usize,
    error: Option<String>,
}

/// One edit the card asks the editor to dispatch after drawing.
#[derive(Clone, Debug, PartialEq)]
enum Edit {
    Set { key: String, value: PropertyValue },
    Clear { key: String },
}

impl PropertiesCard {
    /// Draws the properties of one node and dispatches the accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, node: Id) {
        if node == session.project().root().id() {
            return;
        }
        if self.node != Some(node) {
            self.node = Some(node);
            self.drafts.clear();
            self.new_key.clear();
            self.error = None;
        }
        let Some(properties) = session.project().properties(node).cloned() else {
            return;
        };
        let is_component = session.project().is_component(node);
        let mut edits = Vec::new();

        ui.separator();
        section(ui, "properties", "Properties", true, |ui| {
            ui.horizontal(|ui| {
                let status = if is_component { "Component" } else { "Group" };
                ui.label(RichText::new(status).strong());
                if let Some(furntype) = session.project().furntype(node) {
                    let text =
                        if furntype.is_unclassified() { "unclassified".to_owned() } else { format!("type {furntype}") };
                    ui.label(text);
                }
            });

            if properties.is_empty() {
                ui.label("No properties.");
            }
            for (key, value) in properties.iter() {
                edit_row(ui, key, value, &mut self.drafts, &mut edits);
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.new_key).hint_text("key").desired_width(140.0));
                egui::ComboBox::from_id_salt("properties.new_kind")
                    .selected_text(NEW_KIND_NAMES.get(self.new_kind).copied().unwrap_or("Text"))
                    .show_ui(ui, |ui| {
                        for (index, name) in NEW_KIND_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.new_kind, index, *name);
                        }
                    });
                let can_add = valid_new_key(&self.new_key) && !properties.contains_key(self.new_key.trim());
                if ui.add_enabled(can_add, egui::Button::new("Add")).clicked() {
                    let key = self.new_key.trim().to_owned();
                    edits.push(Edit::Set { key: key.clone(), value: new_value(self.new_kind) });
                    self.drafts.remove(&key);
                    self.new_key.clear();
                }
            });
        });

        for edit in edits {
            if let Err(error) = apply(session, node, edit) {
                self.error = Some(error);
                self.drafts.clear();
                break;
            }
            self.drafts.clear();
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }
}

fn valid_new_key(key: &str) -> bool {
    let key = key.trim();
    !key.is_empty() && !key.chars().any(char::is_whitespace)
}

fn apply(session: &mut Session, node: Id, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::Set { key, value } => {
            let params = json(&NodePropertySet { node, key, value })?;
            dispatch(session, NODE_PROPERTY_SET_ID, &params).map(|_| ())
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
    drafts: &mut BTreeMap<String, String>,
    edits: &mut Vec<Edit>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).strong());
        match value {
            PropertyValue::Bool { value } => {
                let mut checked = *value;
                if ui.checkbox(&mut checked, "").changed() {
                    edits.push(Edit::Set { key: key.to_owned(), value: PropertyValue::bool(checked) });
                }
            }
            PropertyValue::Integer { value } => {
                let mut number = *value;
                if ui.add(DragValue::new(&mut number)).changed() {
                    edits.push(Edit::Set { key: key.to_owned(), value: PropertyValue::integer(number) });
                }
            }
            PropertyValue::Length { value } => {
                let draft = drafts.entry(key.to_owned()).or_insert_with(|| format!("{value}"));
                let response = ui.add(TextEdit::singleline(draft).desired_width(120.0));
                if committed(&response) {
                    match Length::parse_millimetres(draft) {
                        Ok(length) => {
                            drafts.remove(key);
                            edits.push(Edit::Set { key: key.to_owned(), value: PropertyValue::length(length) });
                        }
                        Err(_) => {
                            ui.colored_label(Color32::LIGHT_RED, "mm");
                        }
                    }
                }
            }
            PropertyValue::Angle { value } => {
                let draft = drafts.entry(key.to_owned()).or_insert_with(|| format!("{value}"));
                let response = ui.add(TextEdit::singleline(draft).desired_width(100.0));
                if committed(&response) {
                    match Degrees::parse_degrees(draft) {
                        Ok(angle) => {
                            drafts.remove(key);
                            edits.push(Edit::Set { key: key.to_owned(), value: PropertyValue::angle(angle) });
                        }
                        Err(_) => {
                            ui.colored_label(Color32::LIGHT_RED, "°");
                        }
                    }
                }
            }
            PropertyValue::Text { value } => {
                let draft = drafts.entry(key.to_owned()).or_insert_with(|| value.clone());
                let response = ui.add(TextEdit::singleline(draft).desired_width(180.0));
                if committed(&response) && draft.as_str() != value.as_str() {
                    let text = draft.clone();
                    drafts.remove(key);
                    edits.push(Edit::Set { key: key.to_owned(), value: PropertyValue::text(&text) });
                }
            }
            PropertyValue::FurnType { value } => {
                let draft = drafts.entry(key.to_owned()).or_insert_with(|| value.to_string());
                let response = ui.add(TextEdit::singleline(draft).hint_text("GG.TT.VV").desired_width(100.0));
                if committed(&response) {
                    match draft.parse::<FurnType>() {
                        Ok(furntype) => {
                            drafts.remove(key);
                            edits.push(Edit::Set { key: key.to_owned(), value: PropertyValue::furn_type(furntype) });
                        }
                        Err(_) => {
                            ui.colored_label(Color32::LIGHT_RED, "GG.TT.VV");
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
        if ui.small_button("✕").on_hover_text("Clear property").clicked() {
            edits.push(Edit::Clear { key: key.to_owned() });
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
        apply(&mut session, panel, Edit::Set { key: "finish".to_owned(), value: PropertyValue::text("oak") })
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
}
