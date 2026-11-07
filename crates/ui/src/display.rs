//! Display choices that belong to the workspace, not to the document.
//!
//! These values describe how a viewport draws and which way it faces when it opens. The renderer
//! interprets them in U43; until then they are the single vocabulary the floating cluster and the
//! settings window share, so the two surfaces cannot offer different names for one choice.

use crate::icons;

/// A display style the viewport offers.
///
/// The value is view state: it belongs to the frame the renderer draws, not to the document. The
/// renderer interprets it when the edge and transparency passes land (U43); until then the buttons
/// only remember the choice, which is why the value lives in the settings and the shell rather than
/// in the scene file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadingStyle {
    /// Shaded faces with the silhouette and panel edges drawn over them.
    #[default]
    ShadedWithEdges,
    /// Shaded faces with no edge pass.
    Shaded,
    /// The edge pass alone.
    Wireframe,
    /// Edges drawn through the faces, so hidden geometry stays visible.
    ShadedWithHiddenEdges,
    /// Blended faces with no depth write.
    Translucent,
}

impl ShadingStyle {
    /// Every style in the order the cluster draws them.
    pub const ALL: [Self; 5] =
        [Self::ShadedWithEdges, Self::Shaded, Self::Wireframe, Self::ShadedWithHiddenEdges, Self::Translucent];

    /// The short face used while the icon is still a placeholder.
    pub const fn caption(self) -> &'static str {
        match self {
            Self::ShadedWithEdges => "Edges",
            Self::Shaded => "Shaded",
            Self::Wireframe => "Wire",
            Self::ShadedWithHiddenEdges => "Hidden",
            Self::Translucent => "Ghost",
        }
    }

    /// The full name, used as the tooltip and the accessible name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ShadedWithEdges => "Shaded with edges",
            Self::Shaded => "Shaded",
            Self::Wireframe => "Wireframe",
            Self::ShadedWithHiddenEdges => "Shaded with hidden edges",
            Self::Translucent => "Translucent",
        }
    }

    /// The icon asset that will name the style once it is drawn.
    pub const fn icon(self) -> icons::Icon {
        match self {
            Self::ShadedWithEdges => icons::view::SHADED_EDGES,
            Self::Shaded => icons::view::SHADED,
            Self::Wireframe => icons::view::WIREFRAME,
            Self::ShadedWithHiddenEdges => icons::view::XRAY,
            Self::Translucent => icons::view::TRANSLUCENT,
        }
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
