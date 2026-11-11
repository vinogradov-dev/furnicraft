//! Test helpers the shell's own test modules share.
//!
//! Only a test build compiles this module: it holds the collector that walks an egui frame's shapes
//! for their text, so the status bar, the folded side panel and the strip assert what they drew
//! without a window or a GPU.
//!
//! Changes when: a shell test needs a new way to read what a surface drew.

use eframe::egui;

/// Collects the text of every text shape, including nested ones.
pub(crate) fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
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
