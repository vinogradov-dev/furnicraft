//! Themed icon assets and the icon button.
//!
//! `assets/icons/**` is embedded at compile time, one module per theme. A zero-byte file is a
//! documented placeholder, so an icon button whose artwork has not been drawn yet falls back to its
//! text label and the surface stays usable. A drawn icon is rasterised by the `egui_extras` SVG
//! loader at the pixel size the button draws and tinted with the theme's text colour, so one
//! monochrome master serves every theme (`assets/README.md`, ADR 0047).

use egui::{Button, Color32, Image, Response, Ui, Vec2};

/// Point size at which an icon is drawn, before the UI scale turns it into pixels.
pub const ICON_POINT_SIZE: f32 = 16.0;

/// The directory that owns an icon: the surface that shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Theme {
    /// The Contour toolbar.
    Contour,
    /// The Primitives toolbar.
    Primitives,
    /// The Bodies toolbar.
    Bodies,
    /// The Scene toolbar.
    Scene,
    /// The viewport strip.
    View,
    /// Menus, dialogs, strips and the shortcut reference.
    Actions,
}

impl Theme {
    /// Every theme, in the order of the asset tree.
    pub const ALL: [Self; 6] = [Self::Contour, Self::Primitives, Self::Bodies, Self::Scene, Self::View, Self::Actions];

    /// The directory name of this theme, which is also the first URI segment of its icons.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contour => "contour",
            Self::Primitives => "primitives",
            Self::Bodies => "bodies",
            Self::Scene => "scene",
            Self::View => "view",
            Self::Actions => "actions",
        }
    }

    /// Every icon in this theme, for tests and asset audits.
    pub const fn icons(self) -> &'static [Icon] {
        match self {
            Self::Contour => contour::ALL,
            Self::Primitives => primitives::ALL,
            Self::Bodies => bodies::ALL,
            Self::Scene => scene::ALL,
            Self::View => view::ALL,
            Self::Actions => actions::ALL,
        }
    }
}

/// One embedded icon: its theme, its file stem and its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    theme: Theme,
    name: &'static str,
    uri: &'static str,
    asset: &'static [u8],
}

impl Icon {
    const fn new(theme: Theme, name: &'static str, uri: &'static str, asset: &'static [u8]) -> Self {
        Self { theme, name, uri, asset }
    }

    /// The theme directory that owns this icon.
    pub const fn theme(self) -> Theme {
        self.theme
    }

    /// The file stem, which is the icon id inside its theme.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// The `bytes://` URI the image loader caches the rasterised texture under.
    pub const fn uri(self) -> &'static str {
        self.uri
    }

    /// The embedded SVG file, empty while the icon is a placeholder.
    pub const fn asset(self) -> &'static [u8] {
        self.asset
    }

    /// Whether the asset carries a drawing; a placeholder is an empty file.
    pub const fn is_drawn(self) -> bool {
        !self.asset.is_empty()
    }
}

/// Declares one theme module: every icon file as a constant, plus `ALL` for audits.
macro_rules! icons {
    ($theme:ident, $theme_name:literal, $module:ident { $($constant:ident => $file:literal),* $(,)? }) => {
        #[doc = concat!("The `", $theme_name, "` theme.")]
        pub mod $module {
            use super::{Icon, Theme};

            $(
                #[doc = concat!("The `", $theme_name, "/", $file, "` icon.")]
                pub const $constant: Icon = Icon::new(
                    Theme::$theme,
                    $file,
                    concat!("bytes://icons/", $theme_name, "/", $file, ".svg"),
                    include_bytes!(concat!(
                        "../../../assets/icons/",
                        $theme_name,
                        "/",
                        $file,
                        ".svg"
                    )),
                );
            )*

            /// Every icon in this theme, for tests and asset audits.
            pub const ALL: &[Icon] = &[$($constant),*];
        }
    };
}

icons!(Contour, "contour", contour {
    LINE => "line",
    POLYLINE => "polyline",
    ARC => "arc",
    RECTANGLE => "rectangle",
    CIRCLE => "circle",
    CLOSE => "close",
    CANCEL => "cancel",
});

icons!(Primitives, "primitives", primitives {
    CUBE => "cube",
    CYLINDER => "cylinder",
    SPHERE => "sphere",
    CONE => "cone",
});

icons!(Bodies, "bodies", bodies {
    EXTRUDE => "extrude",
    REVOLVE => "revolve",
    SWEEP => "sweep",
});

icons!(Scene, "scene", scene {
    PANEL => "panel",
    ASSEMBLY => "assembly",
    CAMERA => "camera",
    LIGHT => "light",
});

icons!(View, "view", view {
    ORTHOGRAPHIC => "orthographic",
    PERSPECTIVE => "perspective",
    FRAME_ALL => "frame-all",
});

icons!(Actions, "actions", actions {
    CONFIRM => "confirm",
    DISMISS => "dismiss",
    CANCEL => "cancel",
    UNDO => "undo",
    REDO => "redo",
    DELETE => "delete",
    ADD => "add",
    REMOVE => "remove",
    DUPLICATE => "duplicate",
    RENAME => "rename",
    COPY => "copy",
    PASTE => "paste",
    FILE_NEW => "file-new",
    FILE_OPEN => "file-open",
    FILE_SAVE => "file-save",
    FILE_EXPORT => "file-export",
    FILE_IMPORT => "file-import",
    PRINT => "print",
    SETTINGS => "settings",
    SEARCH => "search",
    VISIBLE => "visible",
    HIDDEN => "hidden",
    LOCK => "lock",
    UNLOCK => "unlock",
    EXPAND => "expand",
    COLLAPSE => "collapse",
    WARNING => "warning",
    ERROR => "error",
    INFO => "info",
    HELP => "help",
});

/// Installs the SVG image loader the icon buttons need.
///
/// The shell calls this once on the context it creates: a URI with no loader keeps showing the
/// loading spinner instead of a drawing.
pub fn install_loaders(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// Draws `icon` as a square button of [`ICON_POINT_SIZE`], tinted with the current text colour.
///
/// An empty placeholder asset draws `label` as the button text instead, so a surface whose artwork
/// is still arriving stays usable. For a drawn icon `label` is the tooltip and the accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    if !icon.is_drawn() {
        return ui.button(label);
    }
    let tint = ui.visuals().text_color();
    let response = ui.add(Button::new(icon_image(tint, icon, label)));
    if label.is_empty() { response } else { response.on_hover_text(label) }
}

/// A tool button: the theme icon with a hint that names the tool and its shortcut.
///
/// The hint is the tooltip of a drawn icon. An icon whose asset is still a placeholder becomes the
/// button text itself, shortcut included, because a tooltip alone would leave the button unreadable.
/// `enabled` disables the button without changing either face, so a panel can keep its layout while
/// a tool is unavailable.
pub fn tool_button(ui: &mut Ui, icon: Icon, label: &str, shortcut: Option<&str>, enabled: bool) -> Response {
    let hint = tool_hint(label, shortcut);
    if !icon.is_drawn() {
        return ui.add_enabled(enabled, Button::new(hint.clone())).on_hover_text(hint);
    }
    let tint = ui.visuals().text_color();
    let response = ui.add_enabled(enabled, Button::new(icon_image(tint, icon, &hint)));
    response.on_hover_text(hint)
}

/// The readable hint of a tool button: the label and, when it has one, its shortcut.
pub fn tool_hint(label: &str, shortcut: Option<&str>) -> String {
    match shortcut {
        Some(shortcut) if !shortcut.is_empty() => format!("{label}  ({shortcut})"),
        _ => label.to_owned(),
    }
}

/// The image atom an icon button carries: the fixed draw size, the theme tint and the alt text.
fn icon_image(tint: Color32, icon: Icon, label: &str) -> Image<'static> {
    Image::from_bytes(icon.uri(), icon.asset())
        .fit_to_exact_size(Vec2::splat(ICON_POINT_SIZE))
        .tint(tint)
        .alt_text(label)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::HashSet;

    use egui::epaint::{ClippedShape, ImageData, Shape};

    use super::*;

    /// A minimal valid icon drawing, used where the real files are still placeholders.
    const SAMPLE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M4 12h16" fill="none" stroke="#000" stroke-width="2"/></svg>"##;

    /// An icon whose asset is empty, like every placeholder in the tree today.
    fn placeholder() -> Icon {
        Icon::new(Theme::Actions, "confirm", "bytes://icons/actions/confirm.svg", &[])
    }

    /// An icon with a real drawing, for the rasteriser tests.
    fn drawn() -> Icon {
        Icon::new(Theme::Actions, "confirm", "bytes://icons/actions/confirm.svg", SAMPLE_SVG)
    }

    /// Collects the text of every text shape, including nested ones.
    fn drawn_text(shapes: &[ClippedShape]) -> Vec<String> {
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
    fn every_icon_uri_names_its_theme_and_file() {
        let mut seen = HashSet::new();
        for theme in Theme::ALL {
            for icon in theme.icons() {
                assert_eq!(icon.uri(), format!("bytes://icons/{}/{}.svg", icon.theme().as_str(), icon.name()));
                assert!(seen.insert(icon.uri()), "duplicate icon uri {}", icon.uri());
            }
        }
        assert_eq!(seen.len(), 51);
    }

    #[test]
    fn a_tool_hint_names_the_shortcut_and_a_placeholder_shows_it() {
        assert_eq!(tool_hint("Line", Some("L")), "Line  (L)");
        assert_eq!(tool_hint("Line", None), "Line");
        assert_eq!(tool_hint("Line", Some("")), "Line");

        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            tool_button(ui, placeholder(), "Line", Some("L"), true);
        });
        let has_hint = drawn_text(&output.shapes).iter().any(|text| text == "Line  (L)");
        output.drop_without_applying_deltas();
        assert!(has_hint, "an empty icon falls back to the labelled shortcut");
    }

    #[test]
    fn a_disabled_tool_button_still_draws_its_label() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            let response = tool_button(ui, placeholder(), "Circle", Some("C"), false);
            assert!(!response.enabled(), "the caller disabled the tool");
        });
        let has_hint = drawn_text(&output.shapes).iter().any(|text| text == "Circle  (C)");
        output.drop_without_applying_deltas();
        assert!(has_hint);
    }

    #[test]
    fn an_empty_asset_draws_the_text_label() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            icon_button(ui, placeholder(), "Confirm");
        });

        let has_label = drawn_text(&output.shapes).iter().any(|text| text == "Confirm");
        output.drop_without_applying_deltas();
        assert!(has_label);
    }

    #[test]
    fn a_drawn_asset_paints_at_the_point_size() {
        let ctx = egui::Context::default();
        install_loaders(&ctx);
        let mut size = Vec2::ZERO;
        let mut pixels_per_point = 1.0;
        let output = ctx.run_ui(Default::default(), |ui| {
            pixels_per_point = ui.pixels_per_point();
            size = ui.add(icon_image(ui.visuals().text_color(), drawn(), "Confirm")).rect.size();
        });

        let has_label = drawn_text(&output.shapes).iter().any(|text| text == "Confirm");
        let expected = (ICON_POINT_SIZE * pixels_per_point).round() as usize;
        let raster = output.textures_delta.set.values().flatten().find_map(|delta| match &delta.image {
            ImageData::Color(image) if image.size == [expected, expected] => Some(image.size),
            _ => None,
        });
        output.drop_without_applying_deltas();

        assert_eq!(size, Vec2::splat(ICON_POINT_SIZE));
        assert!(!has_label);
        assert_eq!(raster, Some([expected, expected]), "the loader rasterised at the button's pixel size");
    }

    #[test]
    fn the_tint_is_the_theme_text_colour() {
        let ctx = egui::Context::default();
        install_loaders(&ctx);
        let mut expected = Color32::PLACEHOLDER;
        let output = ctx.run_ui(Default::default(), |ui| {
            expected = ui.visuals().text_color();
            ui.add(icon_image(expected, drawn(), "Confirm"));
        });

        let tint = output.shapes.iter().find_map(|clipped| match &clipped.shape {
            Shape::Rect(rect) if rect.fill_texture_id() != egui::TextureId::default() => Some(rect.fill),
            _ => None,
        });
        output.drop_without_applying_deltas();

        assert_eq!(tint, Some(expected));
    }
}
