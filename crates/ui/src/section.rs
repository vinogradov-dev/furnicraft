//! Collapsible property sections shared by the inspector cards.
//!
//! A section groups related properties by meaning, like the Transform panel in Blender: the header
//! names the group, and the properties inside can be folded away when the user does not need them.
//! Whether a section is open is view state stored in egui memory; it is never written to the
//! document. The key is explicit so two cards that happen to use the same title do not share a fold
//! state by accident.

use egui::{CollapsingHeader, RichText, Ui};

/// Draws a collapsible section and runs `body` only while it is open.
///
/// `key` is the stable identity of the fold state, `title` is what the user reads. A closed section
/// does not run its body, so a hidden property cannot dispatch an edit.
pub(crate) fn section<R>(
    ui: &mut Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    title: impl Into<RichText>,
    default_open: bool,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    CollapsingHeader::new(title.into().strong()).id_salt(key).default_open(default_open).show(ui, body).body_returned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_section_runs_its_body_and_returns_its_value() {
        egui::__run_test_ui(|ui| {
            let mut ran = false;
            let returned = section(ui, "test.open", "Section", true, |_| {
                ran = true;
                7
            });
            assert!(ran, "an open section must draw its body");
            assert_eq!(returned, Some(7));
        });
    }

    #[test]
    fn a_closed_section_does_not_run_its_body() {
        egui::__run_test_ui(|ui| {
            let mut ran = false;
            let returned = section(ui, "test.closed", "Section", false, |_| {
                ran = true;
                7
            });
            assert!(!ran, "a closed section must not draw its body");
            assert_eq!(returned, None);
        });
    }
}
