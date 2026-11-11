//! The ribbon's map: five tabs, their groups, and the tools and fields inside them.
//!
//! The map is data (`plan/notes/U68.md` holds the approved one). A tab is a noun that names the
//! domain — *Sketch, Model, Furniture, Scene, View* — a group is one operation class under it, a tool
//! is one command and a variant is one input method of that command (U67 draws the list). Every level
//! carries a namespaced id: `sketch`, `sketch.create`, `sketch.create.line`,
//! `sketch.create.circle.three-points`, so a saved layout, a settings value or an agent never depends
//! on a position in the table.
//!
//! The table holds order, words, artwork and ids — never behaviour. What a tool *does* is
//! [`ToolKind`], a small enum the renderer matches once; whether it can run right now is decided
//! against the [`super::RibbonContext`] at draw time. A tool whose command does not exist yet is
//! *planned*: it stays in the table so the map reads whole, and both it and a group of only planned
//! tools are skipped, so a tab with nothing live is not drawn.
//!
//! Every row is a constant, so the ribbon allocates nothing to read the map.

use furnicraft_engine::PrimitiveKind;
use furnicraft_text::Message;

use crate::draw::DrawTool;
use crate::icons::{self, Icon};

use super::shortcut_label;

/// What a click on a tool runs.
///
/// This is the only code a row carries: the map says where a tool sits and what it is called, this
/// says what happens. The data of an action — a primitive's size, an extrusion's height, the selected
/// path — is not stored here; the renderer reads it from the ribbon state and the context when the
/// button is drawn, so a row is one constant for every document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    /// Starts one drawing method.
    Draw(DrawTool),
    /// Closes the running chain into a ring.
    CloseDraft,
    /// Drops the running chain; the document never changed.
    CancelDraft,
    /// Creates a primitive on the next work-plane click.
    Primitive(PrimitiveKind),
    /// Extrudes the selected closed path into a body.
    Extrude,
    /// Creates a panel at the scene root.
    Panel,
    /// Creates an assembly at the scene root.
    Assembly,
    /// Creates a camera at the scene root.
    Camera,
    /// Creates a light at the scene root.
    Light,
    /// Frames the whole scene.
    FrameAll,
    /// Switches between perspective and orthographic.
    Projection,
    /// Turns the work-plane grid snap on or off.
    GridSnap,
}

/// One input method of a tool: what U67 draws as the tool's list.
///
/// The method a cell runs today is [`ToolSpec::run`]; this list holds the others, so the map says
/// "a circle can also be drawn through three points" without repeating the primary row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VariantSpec {
    /// The namespaced id, `tab.group.tool.variant`.
    pub id: &'static str,
    /// The name the list draws.
    pub label: Message,
    /// The artwork, or `None` while the method is planned.
    pub icon: Option<Icon>,
    /// What picking the method runs, or `None` while its command does not exist.
    pub run: Option<ToolKind>,
}

/// One tool: one cell of a group's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    /// The namespaced id, `tab.group.tool`.
    pub id: &'static str,
    /// The name the hint and the accessible label carry.
    pub label: Message,
    /// The artwork, or `None` while the tool is planned.
    pub icon: Option<Icon>,
    /// The shortcut this table prints in the hint, when the tool has one.
    pub shortcut: Option<&'static str>,
    /// The method a click runs today, or `None` while the tool is planned.
    pub run: Option<ToolKind>,
    /// The tool's other input methods, drawn as a list from U67 on.
    pub variants: &'static [VariantSpec],
}

impl ToolSpec {
    /// Whether the tool can run anything today, which is whether it is drawn at all.
    pub fn is_live(&self) -> bool {
        self.run.is_some() || self.variants.iter().any(|variant| variant.run.is_some())
    }

    /// The method a click on the cell runs today.
    pub fn primary(&self) -> Option<ToolKind> {
        self.run.or_else(|| self.variants.iter().find_map(|variant| variant.run))
    }
}

/// The state a tool still holds as a field of the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// The full extent of a new primitive.
    PrimitiveSize,
    /// The height of the next extrusion.
    ExtrudeHeight,
    /// The side count of the regular polygon.
    PolygonSides,
}

/// A numeric field that sits in a group's row next to the tools it configures.
///
/// A field is not a tool: it runs nothing and it is never planned. It takes [`FieldSpec::COLUMNS`]
/// cells of the row, because a number the user types needs more room than a glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// The namespaced id, `tab.group.field`.
    pub id: &'static str,
    /// The name the hint carries; the row itself has no space for a label.
    pub label: Message,
    /// The unit or range drawn as the field's hint text.
    pub hint: &'static str,
    /// Which ribbon field this is.
    pub kind: FieldKind,
}

impl FieldSpec {
    /// The number of cells a field takes in a row.
    pub const COLUMNS: usize = 2;
}

/// One cell of a group: a tool, or the field that configures it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSpec {
    /// A tool button.
    Tool(ToolSpec),
    /// A numeric field.
    Field(FieldSpec),
}

impl CellSpec {
    /// Whether this cell is drawn: a field always is, a tool only once it can run something.
    pub fn is_live(&self) -> bool {
        match self {
            Self::Tool(tool) => tool.is_live(),
            Self::Field(_) => true,
        }
    }

    /// The number of cells this entry takes in the row.
    pub const fn columns(&self) -> usize {
        match self {
            Self::Tool(_) => 1,
            Self::Field(_) => FieldSpec::COLUMNS,
        }
    }
}

/// One captioned group inside a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupSpec {
    /// The namespaced id, `tab.group`.
    pub id: &'static str,
    /// The caption under the group's row, drawn in capitals.
    pub label: Message,
    /// The row's cells in drawing order: the tools, then the fields that configure them.
    pub cells: &'static [CellSpec],
}

impl GroupSpec {
    /// Whether the group draws anything: a group of only planned tools is not drawn.
    pub fn is_live(&self) -> bool {
        self.cells.iter().any(CellSpec::is_live)
    }

    /// The number of cells the group's row takes, fields counted by their width.
    pub fn columns(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_live()).map(CellSpec::columns).sum()
    }

    /// The live tools of the group, in drawing order.
    pub fn tools(&'static self) -> impl Iterator<Item = &'static ToolSpec> {
        self.cells.iter().filter_map(|cell| match cell {
            CellSpec::Tool(tool) if tool.is_live() => Some(tool),
            _ => None,
        })
    }
}

/// One tab of the ribbon: the surface a domain's tools belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabSpec {
    /// The namespaced id, `tab`.
    pub id: &'static str,
    /// The name drawn on the tab button.
    pub label: Message,
    /// The groups in row order.
    pub groups: &'static [GroupSpec],
}

impl TabSpec {
    /// Whether the tab is drawn at all: a tab with no live group is not.
    pub fn is_drawn(&'static self) -> bool {
        self.groups.iter().any(GroupSpec::is_live)
    }

    /// The number of cells the tab's body takes, which is the width of its row in cells.
    pub fn columns(&'static self) -> usize {
        self.live_groups().map(GroupSpec::columns).sum()
    }

    /// The groups the body draws, in order; a group of only planned tools is skipped.
    pub fn live_groups(&'static self) -> impl Iterator<Item = &'static GroupSpec> {
        self.groups.iter().filter(|group| group.is_live())
    }
}

/// Every tab, in ribbon order.
pub static TABS: [TabSpec; 5] = [
    TabSpec { id: "sketch", label: Message::RibbonSketch, groups: &SKETCH },
    TabSpec { id: "model", label: Message::RibbonModel, groups: &MODEL },
    TabSpec { id: "furniture", label: Message::RibbonFurniture, groups: &FURNITURE },
    TabSpec { id: "scene", label: Message::RibbonScene, groups: &SCENE },
    TabSpec { id: "view", label: Message::RibbonView, groups: &VIEW },
];

/// Every tab, in ribbon order.
pub fn tabs() -> &'static [TabSpec] {
    &TABS
}

/// The tab with `id`, or `None` when no such tab exists.
pub fn tab(id: &str) -> Option<&'static TabSpec> {
    TABS.iter().find(|tab| tab.id == id)
}

/// The first tab that draws a body, which is where a fresh ribbon opens and where a ribbon falls
/// back to when its own tab has nothing live.
pub fn fallback_tab() -> Option<&'static TabSpec> {
    TABS.iter().find(|tab| tab.is_drawn())
}

// ── The map ─────────────────────────────────────────────────────────────────

const SKETCH: [GroupSpec; 6] = [
    GroupSpec {
        id: "sketch.create",
        label: Message::GroupCreate,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "sketch.create.line",
                label: Message::ToolLine,
                icon: Some(icons::contour::LINE),
                shortcut: shortcut_label(DrawTool::Line),
                run: Some(ToolKind::Draw(DrawTool::Line)),
                variants: &[VariantSpec {
                    id: "sketch.create.line.angle",
                    label: Message::ToolLineAngle,
                    icon: None,
                    run: None,
                }],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.create.polyline",
                label: Message::ToolPolyline,
                icon: Some(icons::contour::POLYLINE),
                shortcut: shortcut_label(DrawTool::Polyline),
                run: Some(ToolKind::Draw(DrawTool::Polyline)),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.create.rectangle",
                label: Message::ToolRectangle,
                icon: Some(icons::contour::RECTANGLE),
                shortcut: shortcut_label(DrawTool::Rectangle),
                run: Some(ToolKind::Draw(DrawTool::Rectangle)),
                variants: &[
                    VariantSpec {
                        id: "sketch.create.rectangle.centre",
                        label: Message::ToolRectangleFromCentre,
                        icon: Some(icons::contour::RECTANGLE_CENTRE),
                        run: Some(ToolKind::Draw(DrawTool::RectangleFromCenter)),
                    },
                    VariantSpec {
                        id: "sketch.create.rectangle.centre-directed",
                        label: Message::ToolRectangleFromCentreDirected,
                        icon: Some(icons::contour::RECTANGLE_CENTRE_DIRECTED),
                        run: Some(ToolKind::Draw(DrawTool::RectangleFromCenterDirected)),
                    },
                ],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.create.circle",
                label: Message::ToolCircle,
                icon: Some(icons::contour::CIRCLE),
                shortcut: shortcut_label(DrawTool::Circle),
                run: Some(ToolKind::Draw(DrawTool::Circle)),
                variants: &[
                    VariantSpec {
                        id: "sketch.create.circle.two-points",
                        label: Message::ToolCircleTwoPoints,
                        icon: None,
                        run: None,
                    },
                    VariantSpec {
                        id: "sketch.create.circle.three-points",
                        label: Message::ToolCircleThreePoints,
                        icon: Some(icons::contour::CIRCLE_THREE_POINTS),
                        run: Some(ToolKind::Draw(DrawTool::CircleThreePoints)),
                    },
                    VariantSpec {
                        id: "sketch.create.circle.tangent",
                        label: Message::ToolCircleTangent,
                        icon: None,
                        run: None,
                    },
                ],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.create.arc",
                label: Message::ToolArc,
                icon: Some(icons::contour::ARC),
                shortcut: shortcut_label(DrawTool::Arc),
                run: Some(ToolKind::Draw(DrawTool::Arc)),
                variants: &[
                    VariantSpec {
                        id: "sketch.create.arc.centre",
                        label: Message::ToolArcFromCentre,
                        icon: Some(icons::contour::ARC_CENTRE),
                        run: Some(ToolKind::Draw(DrawTool::ArcFromCenter)),
                    },
                    VariantSpec {
                        id: "sketch.create.arc.three-points",
                        label: Message::ToolArcThreePoints,
                        icon: None,
                        run: None,
                    },
                ],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.create.polygon",
                label: Message::ToolRegularPolygon,
                icon: Some(icons::contour::POLYGON),
                shortcut: shortcut_label(DrawTool::RegularPolygon),
                run: Some(ToolKind::Draw(DrawTool::RegularPolygon)),
                variants: &[VariantSpec {
                    id: "sketch.create.polygon.edge",
                    label: Message::ToolPolygonEdge,
                    icon: None,
                    run: None,
                }],
            }),
            CellSpec::Field(FieldSpec {
                id: "sketch.create.sides",
                label: Message::FieldSides,
                hint: "3–128",
                kind: FieldKind::PolygonSides,
            }),
        ],
    },
    GroupSpec {
        id: "sketch.modify",
        label: Message::GroupModify,
        cells: &[
            planned("sketch.modify.trim", Message::ToolTrim),
            planned("sketch.modify.extend", Message::ToolExtend),
            CellSpec::Tool(ToolSpec {
                id: "sketch.modify.offset",
                label: Message::ToolOffset,
                icon: None,
                shortcut: None,
                run: None,
                variants: &[
                    VariantSpec {
                        id: "sketch.modify.offset.distance",
                        label: Message::ToolOffsetDistance,
                        icon: None,
                        run: None,
                    },
                    VariantSpec {
                        id: "sketch.modify.offset.through",
                        label: Message::ToolOffsetThrough,
                        icon: None,
                        run: None,
                    },
                ],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.modify.fillet",
                label: Message::ToolFillet,
                icon: None,
                shortcut: None,
                run: None,
                variants: &[
                    VariantSpec {
                        id: "sketch.modify.fillet.radius",
                        label: Message::ToolFilletRadius,
                        icon: None,
                        run: None,
                    },
                    VariantSpec {
                        id: "sketch.modify.fillet.two-lines",
                        label: Message::ToolFilletTwoLines,
                        icon: None,
                        run: None,
                    },
                ],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.modify.chamfer",
                label: Message::ToolChamfer,
                icon: None,
                shortcut: None,
                run: None,
                variants: &[
                    VariantSpec {
                        id: "sketch.modify.chamfer.distance",
                        label: Message::ToolChamferDistance,
                        icon: None,
                        run: None,
                    },
                    VariantSpec {
                        id: "sketch.modify.chamfer.two-lines",
                        label: Message::ToolChamferTwoLines,
                        icon: None,
                        run: None,
                    },
                ],
            }),
            planned("sketch.modify.break", Message::ToolBreak),
        ],
    },
    GroupSpec {
        id: "sketch.construction",
        label: Message::GroupConstruction,
        cells: &[
            planned("sketch.construction.horizontal", Message::ToolConstructionHorizontal),
            planned("sketch.construction.vertical", Message::ToolConstructionVertical),
            planned("sketch.construction.parallel", Message::ToolConstructionParallel),
            planned("sketch.construction.angle", Message::ToolConstructionAngle),
        ],
    },
    GroupSpec {
        id: "sketch.dimensions",
        label: Message::GroupDimensions,
        cells: &[
            planned("sketch.dimensions.linear", Message::ToolDimensionLinear),
            planned("sketch.dimensions.angular", Message::ToolDimensionAngular),
            CellSpec::Tool(ToolSpec {
                id: "sketch.dimensions.radius",
                label: Message::ToolDimensionRadius,
                icon: None,
                shortcut: None,
                run: None,
                variants: &[VariantSpec {
                    id: "sketch.dimensions.radius.diameter",
                    label: Message::ToolDiameter,
                    icon: None,
                    run: None,
                }],
            }),
        ],
    },
    GroupSpec {
        id: "sketch.booleans",
        label: Message::GroupBooleans,
        cells: &[
            planned("sketch.booleans.union", Message::ToolUnion),
            planned("sketch.booleans.subtract", Message::ToolSubtract),
            planned("sketch.booleans.intersect", Message::ToolIntersect),
        ],
    },
    GroupSpec {
        id: "sketch.draft",
        label: Message::GroupDraft,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "sketch.draft.close",
                label: Message::Close,
                icon: Some(icons::contour::CLOSE),
                shortcut: None,
                run: Some(ToolKind::CloseDraft),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "sketch.draft.cancel",
                label: Message::Cancel,
                icon: Some(icons::contour::CANCEL),
                shortcut: None,
                run: Some(ToolKind::CancelDraft),
                variants: &[],
            }),
        ],
    },
];

const MODEL: [GroupSpec; 5] = [
    GroupSpec {
        id: "model.primitives",
        label: Message::GroupPrimitives,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "model.primitives.cube",
                label: Message::ToolCube,
                icon: Some(icons::primitives::CUBE),
                shortcut: None,
                run: Some(ToolKind::Primitive(PrimitiveKind::Cube)),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "model.primitives.cylinder",
                label: Message::ToolCylinder,
                icon: Some(icons::primitives::CYLINDER),
                shortcut: None,
                run: Some(ToolKind::Primitive(PrimitiveKind::Cylinder)),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "model.primitives.sphere",
                label: Message::ToolSphere,
                icon: Some(icons::primitives::SPHERE),
                shortcut: None,
                run: Some(ToolKind::Primitive(PrimitiveKind::Sphere)),
                variants: &[],
            }),
            planned("model.primitives.cone", Message::ToolCone),
            planned("model.primitives.pyramid", Message::ToolPyramid),
            CellSpec::Field(FieldSpec {
                id: "model.primitives.size",
                label: Message::FieldSize,
                hint: "mm",
                kind: FieldKind::PrimitiveSize,
            }),
        ],
    },
    GroupSpec {
        id: "model.create",
        label: Message::GroupCreate,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "model.create.extrude",
                label: Message::ToolExtrude,
                icon: Some(icons::bodies::EXTRUDE),
                shortcut: None,
                run: Some(ToolKind::Extrude),
                variants: &[],
            }),
            planned("model.create.revolve", Message::ToolRevolve),
            planned("model.create.loft", Message::ToolLoft),
            planned("model.create.sweep", Message::ToolSweep),
            CellSpec::Field(FieldSpec {
                id: "model.create.height",
                label: Message::FieldHeight,
                hint: "mm",
                kind: FieldKind::ExtrudeHeight,
            }),
        ],
    },
    GroupSpec {
        id: "model.booleans",
        label: Message::GroupBooleans,
        cells: &[
            planned("model.booleans.union", Message::ToolUnion),
            planned("model.booleans.subtract", Message::ToolSubtract),
            planned("model.booleans.intersect", Message::ToolIntersect),
        ],
    },
    GroupSpec {
        id: "model.faces",
        label: Message::GroupFaces,
        cells: &[
            planned("model.faces.chamfer", Message::ToolChamfer),
            planned("model.faces.fillet", Message::ToolFillet),
            planned("model.faces.shell", Message::ToolShell),
            planned("model.faces.offset", Message::ToolOffsetFace),
            planned("model.faces.delete", Message::ToolDeleteFace),
        ],
    },
    GroupSpec {
        id: "model.bodies",
        label: Message::GroupBodies,
        cells: &[
            planned("model.bodies.join", Message::ToolJoin),
            planned("model.bodies.split", Message::ToolSplit),
            planned("model.bodies.scale", Message::ToolScale),
            planned("model.bodies.mirror", Message::ToolMirror),
        ],
    },
];

const FURNITURE: [GroupSpec; 5] = [
    GroupSpec {
        id: "furniture.panels",
        label: Message::GroupPanels,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "furniture.panels.panel",
                label: Message::ToolPanel,
                icon: Some(icons::scene::PANEL),
                shortcut: None,
                run: Some(ToolKind::Panel),
                variants: &[],
            }),
            planned("furniture.panels.contour", Message::ToolContourPanel),
        ],
    },
    GroupSpec {
        id: "furniture.machining",
        label: Message::GroupMachining,
        cells: &[
            planned("furniture.machining.drill", Message::ToolDrill),
            planned("furniture.machining.groove", Message::ToolGroove),
            planned("furniture.machining.edgeband", Message::ToolEdgeband),
            planned("furniture.machining.hinge-hole", Message::ToolHingeHole),
        ],
    },
    GroupSpec {
        id: "furniture.fasteners",
        label: Message::GroupFasteners,
        cells: &[
            planned("furniture.fasteners.confirmat", Message::ToolConfirmat),
            planned("furniture.fasteners.cam", Message::ToolCam),
            planned("furniture.fasteners.dowel", Message::ToolDowel),
            planned("furniture.fasteners.minifix", Message::ToolMinifix),
            planned("furniture.fasteners.shelf-pin", Message::ToolShelfPin),
        ],
    },
    GroupSpec {
        id: "furniture.hardware",
        label: Message::GroupHardware,
        cells: &[
            planned("furniture.hardware.hinge", Message::ToolHinge),
            planned("furniture.hardware.runner", Message::ToolRunner),
            planned("furniture.hardware.handle", Message::ToolHandle),
            planned("furniture.hardware.foot", Message::ToolFoot),
        ],
    },
    GroupSpec {
        id: "furniture.checks",
        label: Message::GroupChecks,
        cells: &[
            planned("furniture.checks.collisions", Message::ToolCollisions),
            planned("furniture.checks.parts-list", Message::ToolPartsList),
            planned("furniture.checks.cut-list", Message::ToolCutList),
        ],
    },
];

const SCENE: [GroupSpec; 3] = [
    GroupSpec {
        id: "scene.objects",
        label: Message::GroupObjects,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "scene.objects.camera",
                label: Message::ToolCamera,
                icon: Some(icons::scene::CAMERA),
                shortcut: None,
                run: Some(ToolKind::Camera),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "scene.objects.light",
                label: Message::ToolLight,
                icon: Some(icons::scene::LIGHT),
                shortcut: None,
                run: Some(ToolKind::Light),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "scene.objects.assembly",
                label: Message::ToolAssembly,
                icon: Some(icons::scene::ASSEMBLY),
                shortcut: None,
                run: Some(ToolKind::Assembly),
                variants: &[],
            }),
            planned("scene.objects.group", Message::ToolGroupNode),
        ],
    },
    GroupSpec {
        id: "scene.views",
        label: Message::GroupViews,
        cells: &[
            planned("scene.views.named", Message::ToolNamedView),
            planned("scene.views.standard", Message::ToolStandardView),
        ],
    },
    GroupSpec {
        id: "scene.materials",
        label: Message::GroupMaterials,
        cells: &[
            planned("scene.materials.catalogue", Message::ToolMaterialCatalogue),
            planned("scene.materials.assign", Message::ToolAssignMaterial),
        ],
    },
];

const VIEW: [GroupSpec; 4] = [
    GroupSpec {
        id: "view.projection",
        label: Message::GroupProjection,
        cells: &[
            CellSpec::Tool(ToolSpec {
                id: "view.projection.toggle",
                label: Message::ToolProjection,
                icon: Some(icons::view::ORTHOGRAPHIC),
                shortcut: Some("O"),
                run: Some(ToolKind::Projection),
                variants: &[],
            }),
            CellSpec::Tool(ToolSpec {
                id: "view.projection.frame-all",
                label: Message::ToolFrameAll,
                icon: Some(icons::view::FRAME_ALL),
                shortcut: Some("F"),
                run: Some(ToolKind::FrameAll),
                variants: &[],
            }),
        ],
    },
    GroupSpec {
        id: "view.display",
        label: Message::GroupDisplay,
        cells: &[
            planned("view.display.style", Message::ToolStyle),
            CellSpec::Tool(ToolSpec {
                id: "view.display.grid",
                label: Message::ToolGridSnap,
                icon: Some(icons::view::GRID_SNAP),
                shortcut: Some("G"),
                run: Some(ToolKind::GridSnap),
                variants: &[],
            }),
            planned("view.display.contours", Message::ToolContours),
            planned("view.display.shadows", Message::ToolShadows),
            planned("view.display.section", Message::ToolSection),
        ],
    },
    GroupSpec {
        id: "view.layout",
        label: Message::GroupLayout,
        cells: &[
            planned("view.layout.one", Message::ToolLayoutOne),
            planned("view.layout.two", Message::ToolLayoutTwo),
            planned("view.layout.four", Message::ToolLayoutFour),
            planned("view.layout.maximize", Message::ToolMaximize),
        ],
    },
    GroupSpec {
        id: "view.navigation",
        label: Message::GroupNavigation,
        cells: &[
            planned("view.navigation.orbit", Message::ToolOrbit),
            planned("view.navigation.pan", Message::ToolPan),
            planned("view.navigation.zoom", Message::ToolZoom),
        ],
    },
];

/// A tool whose command does not exist yet: it holds its place and its name in the map.
const fn planned(id: &'static str, label: Message) -> CellSpec {
    CellSpec::Tool(ToolSpec { id, label, icon: None, shortcut: None, run: None, variants: &[] })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::HashSet;

    use super::*;

    /// Every tool of the map, live or planned, with the group it sits in.
    fn every_tool() -> Vec<(&'static GroupSpec, &'static ToolSpec)> {
        let mut tools = Vec::new();
        for tab in tabs() {
            for group in tab.groups {
                for cell in group.cells {
                    if let CellSpec::Tool(tool) = cell {
                        tools.push((group, tool));
                    }
                }
            }
        }
        tools
    }

    #[test]
    fn the_five_tabs_are_the_approved_nouns_in_order() {
        let names = tabs().iter().map(|tab| tab.label.text(furnicraft_text::Lang::En)).collect::<Vec<_>>();
        assert_eq!(names, ["Sketch", "Model", "Furniture", "Scene", "View"]);
        assert_eq!(tabs()[0].id, "sketch");
        assert_eq!(tab("furniture").map(|tab| tab.id), Some("furniture"));
        assert_eq!(tab("hardware"), None, "hardware is a group of furniture, not a tab");
        assert_eq!(fallback_tab().map(|tab| tab.id), Some("sketch"), "a fresh ribbon opens on the first drawn tab");
    }

    #[test]
    fn every_id_is_unique_and_namespaced_by_its_parent() {
        let mut seen = HashSet::new();
        for tab in tabs() {
            assert!(seen.insert(tab.id), "{} is declared twice", tab.id);
            assert!(!tab.id.contains('.'), "a tab id is one word: {}", tab.id);
            for group in tab.groups {
                assert!(
                    group.id.starts_with(tab.id) && group.id.contains('.'),
                    "{} does not sit in {}",
                    group.id,
                    tab.id
                );
                assert!(seen.insert(group.id), "{} is declared twice", group.id);
                for cell in group.cells {
                    let id = match cell {
                        CellSpec::Tool(tool) => tool.id,
                        CellSpec::Field(field) => field.id,
                    };
                    assert!(id.starts_with(group.id), "{id} does not sit in {}", group.id);
                    assert!(seen.insert(id), "{id} is declared twice");
                }
                for cell in group.cells {
                    let CellSpec::Tool(tool) = cell else { continue };
                    for variant in tool.variants {
                        assert!(variant.id.starts_with(tool.id), "{} does not sit in {}", variant.id, tool.id);
                        assert!(seen.insert(variant.id), "{} is declared twice", variant.id);
                    }
                }
            }
        }
    }

    #[test]
    fn every_tool_is_either_live_or_planned_and_a_group_of_only_planned_tools_is_skipped() {
        let (mut live, mut planned_tools) = (0, 0);
        for (_, tool) in every_tool() {
            if tool.is_live() {
                live += 1;
                assert!(tool.primary().is_some(), "{} is live but runs nothing", tool.id);
                assert!(tool.icon.is_some(), "{} is live and has no artwork", tool.id);
            } else {
                planned_tools += 1;
                assert!(tool.primary().is_none());
            }
        }
        assert!(planned_tools > 0, "the map keeps room for the tools that are not built yet");
        assert!(live > 0, "the ribbon draws at least one live tool");
        for tab in tabs() {
            for group in tab.groups {
                assert_eq!(group.is_live(), group.columns() > 0, "{} is live exactly when it draws a cell", group.id);
                if !group.is_live() {
                    assert_eq!(group.tools().count(), 0, "{} is not live but counts a tool", group.id);
                }
            }
        }
        // A tab is drawn exactly when it owns a live group, so a planned-only tab cannot show.
        for tab in tabs() {
            assert_eq!(tab.is_drawn(), tab.live_groups().count() > 0, "{} disagrees with its groups", tab.id);
            if tab.is_drawn() {
                assert!(tab.columns() > 0);
            }
        }
    }

    #[test]
    fn every_live_row_of_the_approved_map_is_on_the_ribbon_today() {
        // The map is the contract: a tab, its groups and the tools the build can run.
        let drawn = |id: &str| tab(id).is_some_and(TabSpec::is_drawn);
        let live = |tab_id: &str| {
            let tab = tab(tab_id).expect("the tab exists");
            let mut ids = tab.live_groups().flat_map(GroupSpec::tools).map(|tool| tool.id).collect::<Vec<_>>();
            ids.sort_unstable();
            ids
        };
        assert!(drawn("sketch") && drawn("model") && drawn("furniture") && drawn("scene") && drawn("view"));
        assert_eq!(
            live("sketch"),
            [
                "sketch.create.arc",
                "sketch.create.circle",
                "sketch.create.line",
                "sketch.create.polygon",
                "sketch.create.polyline",
                "sketch.create.rectangle",
                "sketch.draft.cancel",
                "sketch.draft.close",
            ]
        );
        assert_eq!(
            live("model"),
            ["model.create.extrude", "model.primitives.cube", "model.primitives.cylinder", "model.primitives.sphere",]
        );
        assert_eq!(live("furniture"), ["furniture.panels.panel"]);
        assert_eq!(live("scene"), ["scene.objects.assembly", "scene.objects.camera", "scene.objects.light"]);
        assert_eq!(live("view"), ["view.display.grid", "view.projection.frame-all", "view.projection.toggle"]);
    }

    #[test]
    fn every_tool_kind_appears_exactly_once() {
        let kinds = [
            ToolKind::CloseDraft,
            ToolKind::CancelDraft,
            ToolKind::Extrude,
            ToolKind::Panel,
            ToolKind::Assembly,
            ToolKind::Camera,
            ToolKind::Light,
            ToolKind::FrameAll,
            ToolKind::Projection,
            ToolKind::GridSnap,
            ToolKind::Primitive(PrimitiveKind::Cube),
            ToolKind::Primitive(PrimitiveKind::Cylinder),
            ToolKind::Primitive(PrimitiveKind::Sphere),
            ToolKind::Draw(DrawTool::Line),
            ToolKind::Draw(DrawTool::Polyline),
            ToolKind::Draw(DrawTool::Arc),
            ToolKind::Draw(DrawTool::ArcFromCenter),
            ToolKind::Draw(DrawTool::Rectangle),
            ToolKind::Draw(DrawTool::RectangleFromCenter),
            ToolKind::Draw(DrawTool::RectangleFromCenterDirected),
            ToolKind::Draw(DrawTool::Circle),
            ToolKind::Draw(DrawTool::CircleThreePoints),
            ToolKind::Draw(DrawTool::RegularPolygon),
        ];
        let mut seen = Vec::new();
        for (_, tool) in every_tool() {
            if let Some(kind) = tool.run {
                seen.push((tool.id, kind));
            }
            for variant in tool.variants {
                if let Some(kind) = variant.run {
                    seen.push((variant.id, kind));
                }
            }
        }
        for kind in kinds {
            let holders = seen.iter().filter(|(_, held)| *held == kind).map(|(id, _)| *id).collect::<Vec<_>>();
            assert_eq!(holders.len(), 1, "{kind:?} is reachable from {holders:?}");
        }
        assert_eq!(seen.len(), kinds.len(), "a tool runs a kind the list does not name: {seen:?}");
    }

    #[test]
    fn the_row_is_counted_from_the_spec() {
        let sketch = tab("sketch").expect("sketch");
        // Six creation tools, the side field two cells wide, and the draft pair.
        assert_eq!(sketch.columns(), 10);
        // The planned groups draw nothing, so the Model tab is the three primitives, the size field,
        // the extrusion and its height field.
        assert_eq!(tab("model").expect("model").columns(), 8);
        assert_eq!(tab("furniture").expect("furniture").columns(), 1);
        assert_eq!(tab("scene").expect("scene").columns(), 3);
        assert_eq!(tab("view").expect("view").columns(), 3);
        let widest = tabs().iter().map(TabSpec::columns).max().unwrap_or(0);
        assert_eq!(widest, 10, "the sketch tab is the widest row of the map");
        for tab in tabs() {
            assert!(tab.columns() <= widest);
        }
    }

    #[test]
    fn a_group_draws_its_tools_before_the_field_that_configures_them() {
        let model = tab("model").expect("model");
        let primitives = &model.groups[0];
        let cells = primitives.cells.iter().filter(|cell| cell.is_live()).collect::<Vec<_>>();
        let last = cells.last().expect("the group holds cells");
        assert!(matches!(last, CellSpec::Field(field) if field.kind == FieldKind::PrimitiveSize));
        assert_eq!(primitives.columns(), 5, "three primitives and a two-cell field");
    }
}
