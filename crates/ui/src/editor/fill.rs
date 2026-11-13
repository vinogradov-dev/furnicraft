//! The empty-space tool: the class that fills a niche, and the arm that owns the next click.
//!
//! A post and a shelf are the same gesture with a different class: the class's `fill` trait says
//! which pair of the niche's sides the part takes (ADR 0099), the class names the part in the tree,
//! and the shell measures the void with [`furnicraft_model::Project::niche`] (ADR 0103). The editor
//! only holds *which* class is armed, so the tool survives a document edit and never stores a
//! measurement.
//!
//! Arming refuses twice: no open product means no niche is offered (ADR 0098), and a class without
//! the `fill` trait has no axis pair to take, so the button and the arm cannot disagree about
//! whether the tool can run.
//!
//! Changes when: the fill gesture gains a class, a refusal or a second mode.

use furnicraft_engine::Session;
use furnicraft_model::{ClassCode, Fill};
use furnicraft_text::{Lang, Message};

use super::Editor;

/// The class the next click fills a niche with, and the axis pair its trait names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillArm {
    /// The class a created part carries and takes its label from.
    pub class: ClassCode,
    /// The pair of the niche's sides the part takes; the third is the material's thickness.
    pub fill: Fill,
}

impl Editor {
    /// The armed empty-space tool, or `None` when the next click is not a fill.
    pub fn fill_arm(&self) -> Option<FillArm> {
        self.fill
    }

    /// Arms the empty-space tool with `class`, or refuses with the sentence that says why not.
    ///
    /// A niche belongs to the open product, and a class that does not carry the `fill` trait has no
    /// size pair to take. Arming leaves every other pointer mode: the next click creates a part
    /// rather than starting a chain or placing a primitive.
    pub fn arm_fill(&mut self, session: &Session, class: ClassCode, lang: Lang) -> Result<(), String> {
        if session.active_product().is_none() {
            return Err(Message::FillNeedsProduct.text(lang).to_owned());
        }
        let fill = class
            .traits()
            .and_then(|traits| traits.fill())
            .ok_or_else(|| Message::FillNeedsFillTrait.text(lang).to_owned())?;
        self.mesh_edit = None;
        self.contour_edit = None;
        self.drawing = None;
        self.pending_primitive = None;
        self.box_draft = None;
        self.fill = Some(FillArm { class, fill });
        Ok(())
    }

    /// Drops the empty-space tool; the next click selects again.
    pub fn cancel_fill(&mut self) {
        self.fill = None;
    }
}
