//! The egui shell: panels, widgets and input wiring.
//!
//! Thin by design: actions are dispatched to the engine as commands, and the UI never mutates the document directly.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod outliner;
mod panel_card;

use egui::Ui;
use furnicraft_engine::Session;
use furnicraft_model::Id;

use crate::panel_card::PanelCard;

/// Selection and draft state owned by the editor panels.
#[derive(Debug, Default)]
pub struct Editor {
    selected_panel: Option<Id>,
    card: PanelCard,
}

impl Editor {
    /// Creates an editor with no panel selected; the outliner selects the first panel on first show.
    pub fn new() -> Self {
        Self::default()
    }

    /// The panel currently shown in the card.
    pub const fn selected_panel(&self) -> Option<Id> {
        self.selected_panel
    }

    /// Draws the assembly outliner and reconciles a stale selection.
    pub fn show_outliner(&mut self, ui: &mut Ui, session: &Session) {
        if self.selected_panel.is_none_or(|panel_id| session.project().panel(panel_id).is_none()) {
            self.selected_panel = outliner::first_panel(session.project().root());
        }
        outliner::show(ui, session.project().root(), &mut self.selected_panel);
    }

    /// Draws the selected panel card. Edits are dispatched as engine commands.
    pub fn show_card(&mut self, ui: &mut Ui, session: &mut Session) {
        self.card.show(ui, session, &mut self.selected_panel);
    }
}
