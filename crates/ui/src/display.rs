//! Display choices that belong to the workspace, not to the document.
//!
//! These values describe how a viewport draws and which way it faces when it opens. The renderer
//! interprets them; this module only names them for the cluster and the settings window, so the two
//! surfaces cannot offer different words for one choice.

use crate::icons;

pub use furnicraft_scene::ShadingStyle;

/// The short face of a style, used while the toolbar icon is still a placeholder.
pub const fn shading_caption(style: ShadingStyle) -> &'static str {
    match style {
        ShadingStyle::ShadedWithEdges => "Edges",
        ShadingStyle::Shaded => "Shaded",
        ShadingStyle::Wireframe => "Wire",
        ShadingStyle::ShadedWithHiddenEdges => "Hidden",
        ShadingStyle::Translucent => "Ghost",
    }
}

/// The full name, used as the tooltip and the accessible name.
pub const fn shading_label(style: ShadingStyle) -> &'static str {
    match style {
        ShadingStyle::ShadedWithEdges => "Shaded with edges",
        ShadingStyle::Shaded => "Shaded",
        ShadingStyle::Wireframe => "Wireframe",
        ShadingStyle::ShadedWithHiddenEdges => "Shaded with hidden edges",
        ShadingStyle::Translucent => "Translucent",
    }
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
/// A preset only names an eye direction; the fit distance and the projection stay what the scene
/// computed, so choosing *Top* never moves the model or changes its size. U42 wires the same
/// presets to the split-view bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningView {
    /// The familiar three-quarter view.
    #[default]
    Axonometric,
    /// Looking along `-Y`, from the front.
    Front,
    /// Looking down from `+Z`.
    Top,
    /// Looking from `+X`.
    Right,
}

impl OpeningView {
    /// Every preset in the order the settings window lists them.
    pub const ALL: [Self; 4] = [Self::Axonometric, Self::Front, Self::Top, Self::Right];

    /// The user-visible name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Axonometric => "Axonometric",
            Self::Front => "Front",
            Self::Top => "Top",
            Self::Right => "Right",
        }
    }

    /// The offset from the orbit target to the eye, in world millimetres.
    ///
    /// The shell turns this into its own vector type; keeping the preset as three plain numbers
    /// leaves this crate independent of the geometry crate. The axonometric direction is the one a
    /// fresh camera starts with.
    pub const fn direction(self) -> [f64; 3] {
        match self {
            Self::Axonometric => [0.393, 0.772, 0.500],
            Self::Front => [0.0, -1.0, 0.0],
            Self::Top => [0.0, 0.0, 1.0],
            Self::Right => [1.0, 0.0, 0.0],
        }
    }
}
