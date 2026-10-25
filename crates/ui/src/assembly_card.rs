//! The selected-assembly card.
//!
//! An assembly is an editable group. This card edits its name and shows how many leaves it owns;
//! grouping and ungrouping live in the scene tree so the same action covers several selected nodes.

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::ASSEMBLY_RENAME_ID;
use furnicraft_engine::{AssemblyRename, Session};
use furnicraft_model::Id;

use crate::command::{dispatch, json};

/// Persistent state of the assembly card.
#[derive(Debug, Default)]
pub(crate) struct AssemblyCard {
    draft: Option<Draft>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct Draft {
    assembly: Id,
    name: String,
}

impl AssemblyCard {
    /// Draws the card for the selected assembly and dispatches accepted edits.
    pub(crate) fn show(&mut self, ui: &mut Ui, session: &mut Session, assembly_id: Id) {
        let Some(assembly) = session.project().assembly(assembly_id) else {
            self.draft = None;
            self.error = None;
            ui.label("Select a node in the scene tree.");
            return;
        };
        if self.draft.as_ref().is_none_or(|draft| draft.assembly != assembly_id) {
            self.error = None;
            self.draft = Some(Draft { assembly: assembly_id, name: assembly.name().to_owned() });
        }
        let is_root = assembly_id == session.project().root().id();
        let children = assembly.children().len();
        let Some(draft) = self.draft.as_mut() else {
            return;
        };

        ui.separator();
        ui.label(RichText::new("Name").strong());
        let mut rename = None;
        ui.horizontal(|ui| {
            let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
            let clicked = ui.button("Rename").clicked();
            if response.lost_focus() || clicked {
                let name = draft.name.trim();
                if name.is_empty() {
                    self.error = Some("Assembly name must not be empty.".to_owned());
                } else if name != assembly.name() {
                    rename = Some(name.to_owned());
                }
            }
        });

        ui.separator();
        ui.label(RichText::new("Group").strong());
        ui.label(if is_root {
            "Root assembly; it defines the world frame.".to_owned()
        } else {
            "Editable group of panels and nested assemblies.".to_owned()
        });
        ui.label(format!("Children  {children}"));

        if let Some(name) = rename {
            match rename_assembly(session, assembly_id, name) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
        }

        if let Some(error) = &self.error {
            ui.separator();
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }
}

fn rename_assembly(session: &mut Session, assembly: Id, name: String) -> Result<(), String> {
    let params = json(&AssemblyRename { assembly, name })?;
    dispatch(session, ASSEMBLY_RENAME_ID, &params).map(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    #[test]
    fn renaming_an_assembly_dispatches_through_the_engine() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let assembly = Id::new(3).expect("assembly id");
        rename_assembly(&mut session, assembly, "Drawer box".to_owned()).expect("rename");
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Drawer box");
        session.undo().expect("undo");
        assert_eq!(session.project().assembly(assembly).expect("assembly").name(), "Drawer");
    }
}
