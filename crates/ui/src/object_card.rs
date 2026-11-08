//! The selected geometry object card.
//!
//! An object is a leaf with one geometry payload. A mesh object is edited directly and has no
//! feature history. A primitive object shows its parameters — an edge, a radius, a height, the
//! number of sides and bands — because they are the source of truth and the mesh is derived
//! ([ADR 0054]). A constructive object shows its contour instead: the contour and the feature
//! height are the source of truth, and `object.collapse` bakes it into an editable mesh
//! ([ADR 0037]). A path object shows its flat drawing: it has no mesh and no feature height until
//! it becomes a contour or is extruded.
//!
//! [ADR 0037]: ../docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md
//! [ADR 0054]: ../docs/decisions/0054-a-primitive-keeps-the-parameters-that-built-it.md

use egui::{Color32, RichText, TextEdit, Ui};
use furnicraft_engine::node_cmds::NODE_RENAME_ID;
use furnicraft_engine::object_cmds::{OBJECT_COLLAPSE_ID, OBJECT_CONTOUR_REPLACE_ID, OBJECT_PRIMITIVE_REPLACE_ID};
use furnicraft_engine::{NodeRename, ObjectCollapse, ObjectContourReplace, ObjectPrimitiveReplace, Session};
use furnicraft_model::{Contour, Geometry, Id, Path, Primitive, Project};
use furnicraft_units::Length;

use crate::command::{dispatch, json};
use crate::mesh_edit::{MeshEdit, MeshElement};
use crate::section::section;

/// What the object card asks the editor to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObjectAction {
    /// Nothing was pressed.
    None,
    /// Enter or leave sub-object editing.
    Toggle,
    /// Switch between vertex and face selection.
    Element(MeshElement),
    /// Enter contour editing on this constructive object or path.
    StartContour,
}

/// Most selected vertices the card lists with their positions; the count covers the rest.
const MAX_LISTED_VERTICES: usize = 12;

/// Persistent state of the object card.
#[derive(Debug, Default)]
pub(crate) struct ObjectCard {
    draft: Option<Draft>,
    contour_draft: Option<ContourDraft>,
    primitive_draft: Option<PrimitiveDraft>,
    confirm_collapse: bool,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct Draft {
    object: Id,
    name: String,
}

/// Text being edited and the contour it was read from.
///
/// `base` detects a change made elsewhere, for example by undo: the card re-reads the document
/// instead of letting stale text overwrite a restored contour.
#[derive(Clone, Debug)]
struct ContourDraft {
    object: Id,
    base: Option<Contour>,
    text: String,
}

impl ContourDraft {
    fn new(object: Id, contour: Option<&Contour>) -> Self {
        Self { object, base: contour.cloned(), text: contour.map(format_contour).unwrap_or_default() }
    }
}

/// The editable text of one primitive parameter and the primitive it was read from.
///
/// `base` plays the same role as in [`ContourDraft`]: an edit made elsewhere — by undo, or by a
/// gizmo — re-reads the document instead of leaving stale text in the field.
#[derive(Clone, Debug)]
struct PrimitiveDraft {
    object: Id,
    base: Primitive,
    fields: PrimitiveFields,
    error: Option<String>,
}

/// The parameter fields as typed text, one per editable dimension.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PrimitiveFields {
    edge: String,
    radius: String,
    height: String,
    segments: String,
    bands: String,
}

impl PrimitiveFields {
    fn read(primitive: &Primitive) -> Self {
        let mut fields = Self::default();
        match primitive {
            Primitive::Cube(cube) => fields.edge = cube.edge().to_string(),
            Primitive::Cylinder(cylinder) => {
                fields.radius = cylinder.radius().to_string();
                fields.height = cylinder.height().to_string();
                fields.segments = cylinder.segments().to_string();
            }
            Primitive::Sphere(sphere) => {
                fields.radius = sphere.radius().to_string();
                fields.segments = sphere.segments().to_string();
                fields.bands = sphere.bands().to_string();
            }
        }
        fields
    }
}

impl PrimitiveDraft {
    fn new(object: Id, primitive: &Primitive) -> Self {
        Self { object, base: primitive.clone(), fields: PrimitiveFields::read(primitive), error: None }
    }

    /// The primitive the fields describe, or a readable reason they do not.
    ///
    /// The shape comes from `base`, so a field that belongs to another shape is never invented: a
    /// cube has only an edge and a sphere has no height.
    fn parse(&self) -> Result<Primitive, String> {
        let segments = || parse_count(&self.fields.segments, "sides").map_err(|error| format!("{error}."));
        let bands = || parse_count(&self.fields.bands, "bands").map_err(|error| format!("{error}."));
        let primitive = match &self.base {
            Primitive::Cube(_) => Primitive::cube(parse_length(&self.fields.edge, "edge")?),
            Primitive::Cylinder(_) => Primitive::cylinder(
                parse_length(&self.fields.radius, "radius")?,
                parse_length(&self.fields.height, "height")?,
                segments()?,
            ),
            Primitive::Sphere(_) => {
                Primitive::sphere(parse_length(&self.fields.radius, "radius")?, segments()?, bands()?)
            }
        };
        primitive.map_err(|error| format!("{error}."))
    }
}

fn parse_length(text: &str, field: &str) -> Result<Length, String> {
    Length::parse_millimetres(text.trim()).map_err(|error| format!("The {field}: {error}."))
}

fn parse_count(text: &str, field: &str) -> Result<u32, String> {
    text.trim().parse::<u32>().map_err(|_| format!("The {field} must be a whole number"))
}

/// What the card reports about one object, owned so no document borrow outlives the frame.
#[derive(Clone, Debug, PartialEq)]
struct ObjectSummary {
    name: String,
    kind: &'static str,
    is_mesh: bool,
    vertices: usize,
    faces: usize,
    contour: Option<Contour>,
    path: Option<Path>,
    height: Option<Length>,
    primitive: Option<Primitive>,
}

fn object_summary(project: &Project, id: Id) -> Option<ObjectSummary> {
    let object = project.object(id)?;
    match object.geometry() {
        Geometry::Mesh(mesh) => Some(ObjectSummary {
            name: object.name().to_owned(),
            kind: "Mesh",
            is_mesh: true,
            vertices: mesh.positions().len(),
            faces: mesh.faces().len(),
            contour: None,
            path: None,
            height: None,
            primitive: None,
        }),
        Geometry::Primitive(primitive) => Some(ObjectSummary {
            name: object.name().to_owned(),
            kind: "Primitive",
            is_mesh: false,
            vertices: primitive.vertex_count(),
            faces: primitive.face_count(),
            contour: None,
            path: None,
            height: None,
            primitive: Some(primitive.clone()),
        }),
        Geometry::Constructive(constructive) => Some(ObjectSummary {
            name: object.name().to_owned(),
            kind: "Constructive",
            is_mesh: false,
            vertices: 0,
            faces: 0,
            contour: constructive.contour().ok(),
            path: None,
            height: constructive.height(),
            primitive: None,
        }),
        Geometry::Path(path) => Some(ObjectSummary {
            name: object.name().to_owned(),
            kind: "Path",
            is_mesh: false,
            vertices: 0,
            faces: 0,
            contour: None,
            path: Some(path.clone()),
            height: None,
            primitive: None,
        }),
    }
}

/// Whether the viewport contour edit mode can reshape this object.
///
/// A constructive object stores a ring and a path stores a chain; a mesh or a primitive stores
/// neither.
fn offers_contour_edit(summary: &ObjectSummary) -> bool {
    summary.contour.is_some() || summary.path.is_some()
}

impl ObjectCard {
    /// Draws the card for the selected object and dispatches accepted edits.
    ///
    /// Returns what the user asked to do with sub-object or contour editing; the editor owns both
    /// modes. `contour_editing` tells the card that this object is already being reshaped in the
    /// viewport, so it shows the state instead of the entry button.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        object_id: Id,
        mesh_edit: Option<&MeshEdit>,
        contour_editing: bool,
        single_selection: bool,
    ) -> ObjectAction {
        let Some(summary) = object_summary(session.project(), object_id) else {
            self.draft = None;
            self.contour_draft = None;
            self.confirm_collapse = false;
            self.error = None;
            ui.label("Select a node in the scene tree.");
            return ObjectAction::None;
        };
        if self.draft.as_ref().is_none_or(|draft| draft.object != object_id) {
            self.error = None;
            self.confirm_collapse = false;
            self.contour_draft = None;
            self.primitive_draft = None;
            self.draft = Some(Draft { object: object_id, name: summary.name.clone() });
        }
        let is_constructive = summary.contour.is_some();
        if !is_constructive {
            self.contour_draft = None;
        } else if self
            .contour_draft
            .as_ref()
            .is_none_or(|draft| draft.object != object_id || draft.base != summary.contour)
        {
            self.contour_draft = Some(ContourDraft::new(object_id, summary.contour.as_ref()));
        }
        if let Some(primitive) = &summary.primitive {
            if self.primitive_draft.as_ref().is_none_or(|draft| draft.object != object_id || draft.base != *primitive) {
                self.primitive_draft = Some(PrimitiveDraft::new(object_id, primitive));
            }
        } else {
            self.primitive_draft = None;
        }

        let (rename, name_error) = self.name_section(ui, &summary);
        self.geometry_section(ui, &summary);
        let mut action = self.mesh_section(ui, session, object_id, &summary, mesh_edit, single_selection);
        if self.contour_edit_section(ui, &summary, contour_editing, single_selection) {
            action = ObjectAction::StartContour;
        }
        // The viewport owns the working copy while the mode is live; a text replacement or a
        // collapse would silently make that copy stale.
        let primitive_result = self.primitive_section(ui, &summary);
        let contour_result = if contour_editing { None } else { self.contour_section(ui, &summary) };
        let (collapse, cancel) = if (!is_constructive && summary.primitive.is_none()) || contour_editing {
            (false, false)
        } else {
            self.collapse_section(ui, single_selection)
        };

        if let Some(error) = name_error {
            self.error = Some(error);
        }
        if let Some(new_name) = rename {
            match rename_object(session, object_id, new_name) {
                Ok(()) => {
                    self.error = None;
                    self.draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
            return action;
        }
        if let Some(result) = primitive_result {
            match result.and_then(|primitive| replace_primitive(session, object_id, primitive)) {
                Ok(()) => {
                    self.error = None;
                    self.primitive_draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
            return action;
        }
        if let Some(result) = contour_result {
            match result.and_then(|contour| replace_contour(session, object_id, contour)) {
                Ok(()) => {
                    self.error = None;
                    self.contour_draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
            return action;
        }
        if cancel {
            self.confirm_collapse = false;
        }
        if collapse {
            match collapse_object(session, object_id) {
                Ok(()) => {
                    self.error = None;
                    self.confirm_collapse = false;
                    self.contour_draft = None;
                    self.primitive_draft = None;
                }
                Err(error) => self.error = Some(error),
            }
            ui.ctx().request_repaint();
        }

        if let Some(error) = &self.error {
            ui.separator();
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        action
    }

    fn name_section(&mut self, ui: &mut Ui, summary: &ObjectSummary) -> (Option<String>, Option<String>) {
        let Some(draft) = self.draft.as_mut() else {
            return (None, None);
        };
        let mut rename = None;
        let mut name_error = None;
        ui.separator();
        section(ui, "object.name", "Name", true, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(TextEdit::singleline(&mut draft.name).desired_width(180.0));
                let clicked = ui.button("Rename").clicked();
                if response.lost_focus() || clicked {
                    let trimmed = draft.name.trim();
                    if trimmed.is_empty() {
                        name_error = Some("Object name must not be empty.".to_owned());
                    } else if trimmed != summary.name {
                        rename = Some(trimmed.to_owned());
                    }
                }
            });
        });
        (rename, name_error)
    }

    fn geometry_section(&self, ui: &mut Ui, summary: &ObjectSummary) {
        ui.separator();
        section(ui, "object.geometry", "Geometry", true, |ui| {
            ui.label(RichText::new(summary.kind).strong());
            if let Some(primitive) = &summary.primitive {
                ui.label(primitive.kind().label());
                ui.label(format!("Vertices  {}", summary.vertices));
                ui.label(format!("Faces  {}", summary.faces));
                ui.label("The parameters are the source of truth; the mesh is derived.");
            } else if let Some(contour) = &summary.contour {
                let arcs = contour.segments().filter(|segment| segment.is_arc()).count();
                ui.label(format!("Contour segments  {}", contour.segments().len()));
                if arcs > 0 {
                    ui.label(format!("Arcs  {arcs}"));
                }
                if let Some(height) = summary.height {
                    ui.label(format!("Height  {height}"));
                }
                ui.label("The contour and height are the source of truth; the mesh is derived.");
            } else if let Some(path) = &summary.path {
                let arcs = path.segments().filter(|segment| segment.is_arc()).count();
                ui.label(format!("Path segments  {}", path.segments().len()));
                if arcs > 0 {
                    ui.label(format!("Arcs  {arcs}"));
                }
                ui.label(if path.is_closed() { "Closed chain" } else { "Open chain" });
                ui.label("The drawing is the source of truth; it has no mesh yet.".to_owned());
            } else {
                ui.label(format!("Vertices  {}", summary.vertices));
                ui.label(format!("Faces  {}", summary.faces));
                ui.label("Editable as a mesh; undo is its only history.".to_owned());
            }
        });
    }

    /// Draws the primitive parameters; returns the parsed primitive when Apply was pressed.
    ///
    /// Only the fields the shape has are shown: a cube has one edge, a cylinder a radius, a height
    /// and its sides, a sphere a radius, its sides and its bands. Units come from
    /// `furnicraft-units`, so the text is millimetres with the suffix the user typed.
    fn primitive_section(&mut self, ui: &mut Ui, summary: &ObjectSummary) -> Option<Result<Primitive, String>> {
        let primitive = summary.primitive.as_ref()?;
        let draft = self.primitive_draft.as_mut()?;
        let mut apply = false;
        ui.separator();
        section(ui, "object.primitive", "Parameters", true, |ui| {
            ui.label(format!("{} — edit the parameters and apply.", primitive.kind().label()));
            match primitive {
                Primitive::Cube(_) => {
                    length_field(ui, "Edge", &mut draft.fields.edge);
                }
                Primitive::Cylinder(_) => {
                    length_field(ui, "Radius", &mut draft.fields.radius);
                    length_field(ui, "Height", &mut draft.fields.height);
                    count_field(ui, "Sides", &mut draft.fields.segments);
                }
                Primitive::Sphere(_) => {
                    length_field(ui, "Radius", &mut draft.fields.radius);
                    count_field(ui, "Sides", &mut draft.fields.segments);
                    count_field(ui, "Bands", &mut draft.fields.bands);
                }
            }
            if let Some(error) = &draft.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            apply = ui.button("Apply").clicked();
        });
        if !apply {
            return None;
        }
        let parsed = draft.parse();
        if let Err(error) = &parsed {
            draft.error = Some(error.clone());
            return None;
        }
        Some(parsed)
    }

    fn mesh_section(
        &self,
        ui: &mut Ui,
        session: &Session,
        object_id: Id,
        summary: &ObjectSummary,
        mesh_edit: Option<&MeshEdit>,
        single_selection: bool,
    ) -> ObjectAction {
        ui.separator();
        let mut action = ObjectAction::None;
        section(ui, "object.mesh_edit", "Mesh editing", true, |ui| {
            if summary.contour.is_some() || summary.primitive.is_some() {
                ui.label("Collapse the object to a mesh before editing its elements.");
            } else if !summary.is_mesh {
                ui.label("A path object has no mesh; its drawing is the source of truth.");
            } else if single_selection {
                let editing = mesh_edit.is_some_and(|edit| edit.object() == object_id);
                let label = if editing { "Done" } else { "Edit mesh" };
                if ui.button(label).clicked() {
                    action = ObjectAction::Toggle;
                }
                if let Some(edit) = mesh_edit.filter(|edit| edit.object() == object_id) {
                    let current = edit.element();
                    let mut choice = current;
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut choice, MeshElement::Vertex, "Vertices");
                        ui.selectable_value(&mut choice, MeshElement::Face, "Faces");
                    });
                    if choice != current {
                        action = ObjectAction::Element(choice);
                    }
                }
            } else {
                ui.label("Select only this object to edit its mesh.");
            }
            if let Some(edit) = mesh_edit.filter(|edit| edit.object() == object_id) {
                match edit.element() {
                    MeshElement::Vertex => {
                        ui.label(format!("Selected vertices  {}", edit.vertices().len()));
                        let positions = selected_positions(session.project(), object_id, edit.vertices());
                        for (vertex, position) in positions.iter().take(MAX_LISTED_VERTICES) {
                            ui.label(format!("v{vertex}  {}", format_position(*position)));
                        }
                        if positions.len() > MAX_LISTED_VERTICES {
                            ui.label(format!("… and {} more", positions.len() - MAX_LISTED_VERTICES));
                        }
                        ui.label("Click a vertex in the viewport; Shift or Ctrl adds it to the selection.");
                    }
                    MeshElement::Face => {
                        ui.label(format!("Selected faces  {}", edit.faces().len()));
                        let listed: Vec<String> =
                            edit.faces().iter().take(MAX_LISTED_VERTICES).map(|face| format!("f{face}")).collect();
                        if !listed.is_empty() {
                            ui.label(listed.join("  "));
                        }
                        if edit.faces().len() > MAX_LISTED_VERTICES {
                            ui.label(format!("… and {} more", edit.faces().len() - MAX_LISTED_VERTICES));
                        }
                        ui.label("Click a face in the viewport; Shift or Ctrl adds it to the selection.");
                    }
                }
            }
        });
        action
    }

    /// Draws the entry point into the viewport contour edit mode.
    ///
    /// A constructive object and a path both have a contour or chain to reshape; a mesh or a
    /// primitive object returns `false` and shows nothing. The editor owns the working copy and
    /// validates the object again before it starts.
    fn contour_edit_section(
        &self,
        ui: &mut Ui,
        summary: &ObjectSummary,
        contour_editing: bool,
        single_selection: bool,
    ) -> bool {
        if !offers_contour_edit(summary) {
            return false;
        }
        let mut requested = false;
        ui.separator();
        section(ui, "object.contour_edit", "Contour editing", true, |ui| {
            if contour_editing {
                ui.label("Editing the contour in the viewport. Apply or cancel in the viewport toolbar.");
            } else if !single_selection {
                ui.label("Select only this object to edit its contour.");
            } else {
                ui.label("Drag corners in the viewport; double-click a side to split it.");
                requested = ui.button("Edit contour").clicked();
            }
        });
        requested
    }

    /// Draws the contour editor; returns the parsed replacement when Apply was pressed.
    ///
    /// The text editor writes corners, so it is only shown for a line-only ring; a ring with an arc
    /// cannot be described by corners and says so instead of silently dropping the arc.
    fn contour_section(&mut self, ui: &mut Ui, summary: &ObjectSummary) -> Option<Result<Contour, String>> {
        let contour = summary.contour.as_ref()?;
        if !contour.is_line_only() {
            ui.separator();
            section(ui, "object.contour", "Contour", true, |ui| {
                ui.label("This contour has arcs; corners cannot describe it, so the text editor is off.");
            });
            return None;
        }
        let draft = self.contour_draft.as_mut()?;
        let mut apply = false;
        ui.separator();
        section(ui, "object.contour", "Contour", true, |ui| {
            ui.label(format!(
                "{} points; one per line as `X, Y` in millimetres.",
                contour_corners(contour).map_or(0, |corners| corners.len())
            ));
            ui.add(TextEdit::multiline(&mut draft.text).desired_rows(6).desired_width(220.0));
            if ui.button("Apply contour").clicked() {
                apply = true;
            }
        });
        if !apply {
            return None;
        }
        Some(contour_from_text(&draft.text))
    }

    /// Draws the two-step collapse control; returns whether to collapse or to cancel.
    fn collapse_section(&mut self, ui: &mut Ui, single_selection: bool) -> (bool, bool) {
        let mut collapse = false;
        let mut cancel = false;
        ui.separator();
        section(ui, "object.collapse", "Collapse", true, |ui| {
            if !single_selection {
                ui.label("Select only this object to collapse it.");
                return;
            }
            if self.confirm_collapse {
                ui.colored_label(
                    Color32::from_rgb(230, 180, 80),
                    "Baking the parameters into a mesh deletes them as parameters. Undo is the only way back.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Collapse to mesh").clicked() {
                        collapse = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            } else if ui.button("Collapse to mesh…").clicked() {
                self.confirm_collapse = true;
            }
        });
        (collapse, cancel)
    }
}

/// A labelled millimetre text field.
fn length_field(ui: &mut Ui, label: &str, text: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(TextEdit::singleline(text).desired_width(100.0));
        ui.label("mm");
    });
}

/// A labelled whole-number text field.
fn count_field(ui: &mut Ui, label: &str, text: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(TextEdit::singleline(text).desired_width(100.0));
    });
}

/// The selected vertices that still exist, with their local positions.
fn selected_positions(project: &Project, object: Id, vertices: &[u32]) -> Vec<(u32, [Length; 3])> {
    let Some(mesh) = project.object(object).and_then(|object| object.geometry().as_mesh()) else {
        return Vec::new();
    };
    vertices
        .iter()
        .filter_map(|vertex| {
            let index = usize::try_from(*vertex).ok()?;
            mesh.positions().get(index).map(|position| (*vertex, *position))
        })
        .collect()
}

fn format_position(position: [Length; 3]) -> String {
    format!("{}  {}  {}", position[0], position[1], position[2])
}

fn format_contour(contour: &Contour) -> String {
    contour_corners(contour)
        .unwrap_or_default()
        .iter()
        .map(|point| format!("{}, {}", point[0], point[1]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The corner points of a line-only ring, in travel order; `None` when a segment is an arc.
///
/// The text editor writes corners, so a ring with an arc has no corner list to show: sampling it
/// would silently replace the arc with straight lines on the next Apply.
fn contour_corners(contour: &Contour) -> Option<Vec<[Length; 2]>> {
    contour
        .segments()
        .map(|segment| match segment {
            furnicraft_model::ContourSegment::Line { start, .. } => Some(start),
            furnicraft_model::ContourSegment::Arc { .. } => None,
        })
        .collect()
}

/// Parses one `X, Y` line per contour point, in millimetres.
fn contour_from_text(text: &str) -> Result<Contour, String> {
    let mut points = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((x, y)) = line.split_once(',') else {
            return Err(format!("Line {}: expected `X, Y` in millimetres.", index.saturating_add(1)));
        };
        let x = Length::parse_millimetres(x.trim())
            .map_err(|error| format!("Line {}: {error}.", index.saturating_add(1)))?;
        let y = Length::parse_millimetres(y.trim())
            .map_err(|error| format!("Line {}: {error}.", index.saturating_add(1)))?;
        points.push([x, y]);
    }
    Contour::from_polygon(points).map_err(|error| format!("{error}."))
}

fn rename_object(session: &mut Session, node: Id, name: String) -> Result<(), String> {
    let params = json(&NodeRename { node, name })?;
    dispatch(session, NODE_RENAME_ID, &params).map(|_| ())
}

fn replace_contour(session: &mut Session, object: Id, contour: Contour) -> Result<(), String> {
    let params = json(&ObjectContourReplace { object, path: contour.into() })?;
    dispatch(session, OBJECT_CONTOUR_REPLACE_ID, &params).map(|_| ())
}

fn replace_primitive(session: &mut Session, object: Id, primitive: Primitive) -> Result<(), String> {
    let params = json(&ObjectPrimitiveReplace { object, primitive })?;
    dispatch(session, OBJECT_PRIMITIVE_REPLACE_ID, &params).map(|_| ())
}

fn collapse_object(session: &mut Session, object: Id) -> Result<(), String> {
    let params = json(&ObjectCollapse { object })?;
    dispatch(session, OBJECT_COLLAPSE_ID, &params).map(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_engine::ObjectExtrudeCreate;
    use furnicraft_engine::object_cmds::OBJECT_EXTRUDE_CREATE_ID;
    use furnicraft_model::{Mesh, Placement};
    use furnicraft_testkit::{sample_catalog, sample_project};
    use furnicraft_units::Length;

    use super::*;

    fn rectangle(width: i64, height: i64) -> Contour {
        let millimetres = |value| Length::from_millimetres(value).expect("length");
        Contour::from_polygon(vec![
            [millimetres(0), millimetres(0)],
            [millimetres(width), millimetres(0)],
            [millimetres(width), millimetres(height)],
            [millimetres(0), millimetres(height)],
        ])
        .expect("rectangle")
    }

    fn object_project() -> (furnicraft_model::Project, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let mesh = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        let id = project.add_object(root, "Cube", Placement::IDENTITY, Geometry::Mesh(mesh)).expect("add object");
        (project, id)
    }

    fn constructive_session() -> (Session, Id) {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let value = json(&ObjectExtrudeCreate {
            parent: session.project().root().id(),
            name: "Top".to_owned(),
            contour: rectangle(100, 50),
            height: Length::from_millimetres(18).expect("height"),
            placement: Placement::IDENTITY,
        })
        .expect("params");
        let id = dispatch(&mut session, OBJECT_EXTRUDE_CREATE_ID, &value).expect("create").created_id().expect("id");
        (session, id)
    }

    #[test]
    fn summary_reports_a_mesh_object() {
        let (project, id) = object_project();
        let summary = object_summary(&project, id).expect("summary");
        assert_eq!(summary.kind, "Mesh");
        assert_eq!(summary.vertices, 8);
        assert_eq!(summary.faces, 6);
        assert_eq!(summary.name, "Cube");
        assert_eq!(summary.contour, None);
        assert!(!offers_contour_edit(&summary), "a baked mesh has no contour or chain to reshape");
    }

    #[test]
    fn summary_reports_a_constructive_object() {
        let (session, id) = constructive_session();
        let summary = object_summary(session.project(), id).expect("summary");
        assert_eq!(summary.kind, "Constructive");
        assert_eq!(summary.contour, Some(rectangle(100, 50)));
        assert_eq!(summary.height, Some(Length::from_millimetres(18).expect("height")));
        assert!(summary.contour.as_ref().is_some_and(|contour| contour.segments().len() == 4));
        assert!(offers_contour_edit(&summary), "a constructive object has a ring to reshape");
    }

    #[test]
    fn summary_reports_a_primitive_object_with_its_parameters() {
        let mut project = sample_project();
        let root = project.root().id();
        let primitive = Primitive::sphere(Length::from_millimetres(40).expect("radius"), 24, 12).expect("sphere");
        let id = project
            .add_object(root, "Ball", Placement::IDENTITY, Geometry::Primitive(primitive.clone()))
            .expect("add object");

        let summary = object_summary(&project, id).expect("summary");
        assert_eq!(summary.kind, "Primitive");
        assert!(!summary.is_mesh, "the mesh is derived, not stored");
        assert_eq!(summary.primitive, Some(primitive));
        assert_eq!(summary.contour, None);
        assert_eq!(summary.path, None);
        assert!(!offers_contour_edit(&summary), "a primitive stores parameters, not a contour");
        assert_eq!(summary.faces, 24 * 12);
        assert_eq!(summary.vertices, 2 + 24 * 11);
    }

    fn primitive_session(primitive: Primitive) -> (Session, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let id = project.add_object(root, "Shape", Placement::IDENTITY, Geometry::Primitive(primitive)).expect("add");
        let session = Session::with_catalog(project, sample_catalog());
        (session, id)
    }

    #[test]
    fn the_primitive_fields_read_and_parse_the_stored_parameters() {
        let cases = [
            Primitive::cube(Length::from_millimetres(100).expect("edge")).expect("cube"),
            Primitive::cylinder(
                Length::from_millimetres(25).expect("radius"),
                Length::from_millimetres(80).expect("height"),
                20,
            )
            .expect("cylinder"),
            Primitive::sphere(Length::from_millimetres(40).expect("radius"), 24, 12).expect("sphere"),
        ];
        for primitive in cases {
            let draft = PrimitiveDraft::new(Id::new(1).expect("id"), &primitive);
            assert_eq!(draft.fields, PrimitiveFields::read(&primitive), "{primitive:?} reads back");
            assert_eq!(draft.parse().as_ref(), Ok(&primitive), "{primitive:?} parses from its own text");
        }
    }

    #[test]
    fn the_primitive_fields_refuse_bad_text_and_out_of_range_values() {
        let cube = Primitive::cube(Length::from_millimetres(100).expect("edge")).expect("cube");
        let mut draft = PrimitiveDraft::new(Id::new(1).expect("id"), &cube);
        draft.fields.edge = "wide".to_owned();
        assert!(draft.parse().is_err(), "text must parse as a length");
        draft.fields.edge = "0".to_owned();
        assert!(draft.parse().is_err(), "a zero edge is not a solid");

        let sphere = Primitive::sphere(Length::from_millimetres(40).expect("radius"), 24, 12).expect("sphere");
        let mut draft = PrimitiveDraft::new(Id::new(1).expect("id"), &sphere);
        draft.fields.segments = "two".to_owned();
        assert!(draft.parse().is_err(), "the sides must be a whole number");
        draft.fields.segments = "2".to_owned();
        assert!(draft.parse().is_err(), "two sides are not a ring");
        draft.fields.segments = "24".to_owned();
        draft.fields.bands = "1".to_owned();
        assert!(draft.parse().is_err(), "one band has no surface");
    }

    #[test]
    fn applying_new_parameters_is_one_command_and_keeps_the_id() {
        let original = Primitive::cube(Length::from_millimetres(100).expect("edge")).expect("cube");
        let (mut session, id) = primitive_session(original.clone());
        let before = session.project().revision();
        let wider = Primitive::cube(Length::from_millimetres(160).expect("edge")).expect("cube");
        replace_primitive(&mut session, id, wider.clone()).expect("replace the parameters");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"), "one revision per apply");
        assert_eq!(session.project().object_primitive(id), Ok(&wider));
        assert_eq!(session.project().object(id).expect("object").name(), "Shape", "only the parameters change");

        session.undo().expect("undo");
        assert_eq!(session.project().object_primitive(id), Ok(&original));
    }

    #[test]
    fn collapsing_a_primitive_is_undoable() {
        let primitive = Primitive::sphere(Length::from_millimetres(40).expect("radius"), 16, 8).expect("sphere");
        let (mut session, id) = primitive_session(primitive);
        collapse_object(&mut session, id).expect("collapse");
        let object = session.project().object(id).expect("object");
        assert!(object.geometry().as_mesh().is_some(), "collapse bakes the derived mesh");
        assert!(object.geometry().as_primitive().is_none(), "the parameters are dropped");
        assert!(collapse_object(&mut session, id).is_err(), "a baked mesh cannot collapse again");

        session.undo().expect("undo");
        assert!(session.project().object(id).expect("object").geometry().as_primitive().is_some());
    }

    #[test]
    fn an_unknown_id_has_no_summary() {
        let (project, _) = object_project();
        assert_eq!(object_summary(&project, Id::new(99).expect("id")), None);
    }

    #[test]
    fn summary_reports_a_path_object_as_a_flat_drawing() {
        use furnicraft_model::{ContourSegment, Path};

        let mut project = sample_project();
        let root = project.root().id();
        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let path = Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)])
                    .expect("line"),
                ContourSegment::line([millimetres(100), millimetres(0)], [millimetres(100), millimetres(50)])
                    .expect("line"),
            ],
            false,
        )
        .expect("open path");
        let id = project.add_object(root, "Sketch", Placement::IDENTITY, Geometry::Path(path)).expect("add");

        let summary = object_summary(&project, id).expect("summary");
        assert_eq!(summary.kind, "Path");
        assert!(!summary.is_mesh, "a path has no editable mesh");
        assert_eq!(summary.contour, None);
        assert_eq!(summary.path.as_ref().map(|path| path.segments().len()), Some(2));
        assert_eq!(summary.path.as_ref().map(Path::is_closed), Some(false));
        assert!(offers_contour_edit(&summary), "a path chain is reshaped in the same viewport mode");
    }

    #[test]
    fn selected_positions_skip_indices_the_mesh_no_longer_has() {
        let (project, id) = object_project();
        let positions = selected_positions(&project, id, &[0, 3, 8]);
        assert_eq!(positions.len(), 2, "vertex 8 is outside a cube");
        assert_eq!(positions[0].0, 0);
        assert_eq!(positions[1].0, 3);
        let cube = Mesh::cube(Length::from_millimetres(100).expect("length")).expect("cube");
        assert_eq!(positions[0].1, cube.positions()[0]);
        assert_eq!(positions[1].1, cube.positions()[3]);
        assert!(format_position(positions[0].1).ends_with(" mm"));
    }

    #[test]
    fn a_contour_text_round_trips_and_rejects_bad_lines() {
        let contour = rectangle(100, 50);
        let text = format_contour(&contour);
        assert_eq!(contour_from_text(&text), Ok(contour));

        assert!(contour_from_text("0, 0\n100, 0").is_err(), "a contour needs three points");
        assert!(contour_from_text("wide, 0\n100, 0\n100, 50").is_err(), "text must parse");
        assert!(contour_from_text("0 0\n100 0\n100 50").is_err(), "a comma separates the axes");
        assert!(contour_from_text("0, 0\n30, 0\n20, 20\n10, -10").is_err(), "a self-intersecting contour is refused");
    }

    #[test]
    fn a_contour_with_an_arc_has_no_corner_text() {
        use furnicraft_model::ContourSegment;
        use furnicraft_units::Degrees;

        let millimetres = |value: i64| Length::from_millimetres(value).expect("length");
        let arc = ContourSegment::arc(
            [Length::ZERO, Length::ZERO],
            millimetres(100),
            Degrees::ZERO,
            Degrees::from_millidegrees(90_000),
        )
        .expect("quarter arc");
        let contour = Contour::new(vec![
            arc,
            ContourSegment::line([Length::ZERO, millimetres(100)], [Length::ZERO, Length::ZERO]).expect("line"),
            ContourSegment::line([Length::ZERO, Length::ZERO], [millimetres(100), Length::ZERO]).expect("line"),
        ])
        .expect("a quarter disc");
        assert!(!contour.is_line_only());
        assert_eq!(contour_corners(&contour), None, "corners cannot describe the arc");
        assert_eq!(format_contour(&contour), "", "the text editor has nothing to show");
    }

    #[test]
    fn renaming_an_object_dispatches_through_the_engine() {
        let (project, id) = object_project();
        let mut session = Session::with_catalog(project, sample_catalog());
        rename_object(&mut session, id, "Bracket".to_owned()).expect("rename");
        assert_eq!(session.project().object(id).expect("object").name(), "Bracket");
        session.undo().expect("undo");
        assert_eq!(session.project().object(id).expect("object").name(), "Cube");
    }

    #[test]
    fn replacing_the_contour_is_one_command_and_keeps_the_id() {
        let (mut session, id) = constructive_session();
        let before = session.project().revision();
        replace_contour(&mut session, id, rectangle(120, 60)).expect("replace");
        assert_eq!(session.project().revision(), before.checked_next().expect("revision"));
        let object = session.project().object(id).expect("object");
        assert_eq!(object.geometry().as_constructive().expect("constructive").contour(), Ok(rectangle(120, 60)));
        session.undo().expect("undo");
        assert_eq!(
            session.project().object(id).expect("object").geometry().as_constructive().expect("constructive").contour(),
            Ok(rectangle(100, 50))
        );
    }

    #[test]
    fn collapsing_a_constructive_object_is_undoable() {
        let (mut session, id) = constructive_session();
        collapse_object(&mut session, id).expect("collapse");
        let object = session.project().object(id).expect("object");
        assert!(object.geometry().as_mesh().is_some());
        assert!(object.geometry().as_constructive().is_none());
        assert!(collapse_object(&mut session, id).is_err(), "a mesh object cannot collapse again");
        session.undo().expect("undo");
        assert!(session.project().object(id).expect("object").geometry().as_constructive().is_some());
    }

    #[test]
    fn extrude_parameters_create_the_object_the_card_expects() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let value = json(&ObjectExtrudeCreate {
            parent: session.project().root().id(),
            name: "Top".to_owned(),
            contour: rectangle(100, 50),
            height: Length::from_millimetres(18).expect("height"),
            placement: Placement::IDENTITY,
        })
        .expect("params");
        let id = dispatch(&mut session, OBJECT_EXTRUDE_CREATE_ID, &value).expect("create").created_id().expect("id");
        let summary = object_summary(session.project(), id).expect("summary");
        assert_eq!(summary.kind, "Constructive");
        assert_eq!(summary.height, Some(Length::from_millimetres(18).expect("height")));
    }
}
