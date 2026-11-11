//! Display choices that belong to the workspace, not to the document.
//!
//! These values describe how a viewport draws and which way it faces when it opens. The renderer
//! interprets them; this module only names them for the cluster and the settings window, so the two
//! surfaces cannot offer different words for one choice.

use crate::icons;
use furnicraft_text::{Lang, Message};

pub use furnicraft_scene::ShadingStyle;

/// The short face of a style, used while the toolbar icon is still a placeholder.
pub const fn shading_caption(style: ShadingStyle, lang: Lang) -> &'static str {
    match style {
        ShadingStyle::ShadedWithEdges => Message::ShadingEdgesCaption,
        ShadingStyle::Shaded => Message::ShadingShadedCaption,
        ShadingStyle::Wireframe => Message::ShadingWireCaption,
        ShadingStyle::ShadedWithHiddenEdges => Message::ShadingHiddenCaption,
        ShadingStyle::Translucent => Message::ShadingGhostCaption,
    }
    .text(lang)
}

/// The full name, used as the tooltip and the accessible name.
pub const fn shading_label(style: ShadingStyle, lang: Lang) -> &'static str {
    match style {
        ShadingStyle::ShadedWithEdges => Message::ShadingShadedWithEdges,
        ShadingStyle::Shaded => Message::ShadingShaded,
        ShadingStyle::Wireframe => Message::ShadingWireframe,
        ShadingStyle::ShadedWithHiddenEdges => Message::ShadingHiddenEdges,
        ShadingStyle::Translucent => Message::ShadingTranslucent,
    }
    .text(lang)
}

/// The icon asset that names the style once it is drawn.
pub const fn shading_icon(style: ShadingStyle) -> icons::Icon {
    match style {
        ShadingStyle::ShadedWithEdges => icons::view::SHADED_EDGES,
        ShadingStyle::Shaded => icons::view::SHADED,
        ShadingStyle::Wireframe => icons::view::WIREFRAME,
        ShadingStyle::ShadedWithHiddenEdges => icons::view::XRAY,
        ShadingStyle::Translucent => icons::view::TRANSLUCENT,
    }
}

/// The preset a viewport opens with.
///
/// A preset only names an eye direction and, for the five standard views, the orthographic
/// projection. Applying one never moves the target or changes the distance, so choosing *Top*
/// never moves the model or changes its size. U42 wires the same presets to the split-view bar.
///
/// The standard directions follow the owner's eye convention: `Front` is the view from `+Y`
/// looking towards `-Y`, so screen right is `-X`; `Left` looks from `+X` to `-X`; `Right` looks from
/// `-X` to `+X`; `Top` looks down from `+Z`; `Bottom` looks up from `-Z`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningView {
    /// The familiar three-quarter view.
    #[default]
    Axonometric,
    /// Looking from `+Y` towards `-Y`.
    Front,
    /// Looking from `+X` towards `-X`.
    Left,
    /// Looking down from `+Z`.
    Top,
    /// Looking from `-X` towards `+X`.
    Right,
    /// Looking up from `-Z`.
    Bottom,
}

impl OpeningView {
    /// Every preset in the order the settings window lists them.
    pub const ALL: [Self; 6] = [Self::Axonometric, Self::Front, Self::Left, Self::Top, Self::Right, Self::Bottom];

    /// The user-visible name.
    pub const fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Axonometric => Message::Axonometric,
            Self::Front => Message::Front,
            Self::Left => Message::Left,
            Self::Top => Message::Top,
            Self::Right => Message::Right,
            Self::Bottom => Message::Bottom,
        }
        .text(lang)
    }

    /// The offset from the orbit target to the eye, in world millimetres.
    ///
    /// The shell turns this into its own vector type; keeping the preset as three plain numbers
    /// leaves this crate independent of the geometry crate. The axonometric direction is the one a
    /// fresh camera starts with.
    pub const fn direction(self) -> [f64; 3] {
        match self {
            Self::Axonometric => [0.393, 0.772, 0.500],
            Self::Front => [0.0, 1.0, 0.0],
            Self::Left => [1.0, 0.0, 0.0],
            Self::Top => [0.0, 0.0, 1.0],
            Self::Right => [-1.0, 0.0, 0.0],
            Self::Bottom => [0.0, 0.0, -1.0],
        }
    }

    /// Whether this is one of the five standard views rather than the axonometric default.
    ///
    /// A standard view is orthographic by default and a click on a world axis snap is the same
    /// kind of view; the axonometric view keeps the projection the camera already had.
    pub const fn is_standard(self) -> bool {
        !matches!(self, Self::Axonometric)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_a_label_and_an_eye_direction() {
        assert_eq!(OpeningView::ALL.len(), 6, "the six views the owner names");
        let mut labels = Vec::new();
        for view in OpeningView::ALL {
            assert!(!view.label(Lang::En).is_empty(), "{view:?} has a name");
            assert!(!labels.contains(&view.label(Lang::En)), "{view:?} owns its name");
            labels.push(view.label(Lang::En));
            let [x, y, z] = view.direction();
            let length = (x * x + y * y + z * z).sqrt();
            // The axonometric direction is the three-quarter default, given to three decimals;
            // the standard views are exact axes.
            let tolerance = if view.is_standard() { 1.0e-9 } else { 1.0e-2 };
            assert!((length - 1.0).abs() < tolerance, "{view:?} names a unit direction: {length}");
        }
        assert_eq!(labels, ["Axonometric", "Front", "Left", "Top", "Right", "Bottom"]);
    }

    #[test]
    fn the_standard_views_follow_the_owner_eye_convention() {
        assert_eq!(OpeningView::Front.direction(), [0.0, 1.0, 0.0], "Front looks from +Y to -Y");
        assert_eq!(OpeningView::Left.direction(), [1.0, 0.0, 0.0], "Left looks from +X to -X");
        assert_eq!(OpeningView::Right.direction(), [-1.0, 0.0, 0.0], "Right looks from -X to +X");
        assert_eq!(OpeningView::Top.direction(), [0.0, 0.0, 1.0], "Top looks down from +Z");
        assert_eq!(OpeningView::Bottom.direction(), [0.0, 0.0, -1.0], "Bottom looks up from -Z");
        assert_eq!(OpeningView::Axonometric.direction(), [0.393, 0.772, 0.500]);
    }

    #[test]
    fn the_five_axis_views_are_standard_and_axonometric_is_not() {
        for view in OpeningView::ALL {
            assert_eq!(view.is_standard(), view != OpeningView::Axonometric);
        }
        assert_eq!(OpeningView::ALL.iter().filter(|view| view.is_standard()).count(), 5);
    }
}
