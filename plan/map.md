# Map

One row per feature: where it lives and the anchor to grep for. Before reading a large file, find
the feature here and read the range around its anchor, not the whole file. Update a row only when
the feature moves — a new module or a rename — not after every task. The dashboard prints this file
as a `MAP` block.
The plan gate does not check this file; a stale row is a defect the reader fixes.

| Feature | Where | Notes |
|---|---|---|
| Contour ring, segment ids | `crates/model/src/contour.rs` (`Contour`, `ContourSegmentId`, `assemble`) | whole file |
| Flat path value | `crates/model/src/contour.rs` (`Path`, `PathError`, `polyline`, `to_contour`, `creation_pivot`), `crates/model/src/project/object_cmds.rs` (`add_object_with_pivot`) | open or closed chain; `to_contour` applies the ring rules and keeps the ids; a new figure pivots at its bounds centre or its first point (ADR 0058) |
| Path line primitive | `crates/scene/src/lib.rs` (`Polyline`, `SceneFrame::push_polyline`, `SceneFrame::pick`), `crates/render/src/lib.rs` (`polyline_vertices`), `crates/mesh/src/lib.rs` (`path_points`) | unlit chain; the renderer's line pipeline; a pick hits a segment within `LINE_PICK_RADIUS_POINTS` |
| Contour edit math | `crates/model/src/contour_edit.rs` (`move_corner`, `trim_segment`, `split_segment`, `PathCorner`, the `Path` family) | whole file; a ring addresses a corner by the segment that starts on it, an open chain by `PathCorner` (ADR 0060) |
| Contour edit mode | `crates/ui/src/contour_edit.rs` (`ContourEdit`, `ContourTarget`), `crates/ui/src/lib.rs` (`Editor`), `apps/furnicraft/src/contour_gizmo.rs` (`ContourGizmo`), `apps/furnicraft/src/main.rs` | whole files; the target is a panel ring, a constructive contour, a `Path` or a cutout ring |
| Selection outline | `crates/scene/src/lib.rs` (`SELECTION_LINE_COLOR`, `with_selection_highlight`), `apps/furnicraft/src/main.rs` (`upload_scene`, `selection_outline`) | a selected chain gets an explicit colour and is repainted as a screen-space outline, because a `LineList` cannot be widened (ADR 0059) |
| Drawing draft | `crates/ui/src/contour_edit.rs` (`DrawDraft`, `DrawTool`, `DrawDraftError`, `DrawDraft::commit`), `crates/ui/src/lib.rs` (`Editor::start_drawing`, `drawing_point`, `commit_drawing`) | view state until the one `object.path.create`; `DrawnPath` is the result |
| Drawing tool preview | `apps/furnicraft/src/draw_gizmo.rs` (`DrawGizmo`, `snap_cursor`, `click_action`, `size_label`), `apps/furnicraft/src/main.rs` (`viewport`, strip) | owns the pointer while a draft runs; no document change |
| Primitive placement | `apps/furnicraft/src/gizmo_math.rs` (`WorkPlane::placement_at`), `apps/furnicraft/src/main.rs` (`viewport`, `placing`), `crates/ui/src/toolbars.rs` (`Toolbars::primitive_size`), `crates/ui/src/lib.rs` (`Editor::arm_primitive`, `place_primitive`) | a primitive tool arms, the next work-plane click creates one `object.primitive.create` |
| Primitive parameters | `crates/model/src/primitive.rs` (`Primitive`, `Cube`, `Cylinder`, `Sphere`, `PrimitiveKind`, the size and tessellation caps), `crates/model/src/object.rs` (`Geometry::Primitive`), `crates/engine/src/object_cmds.rs` (`ObjectPrimitiveCreate`, `ObjectPrimitiveReplace`) | the parameters are the source of truth; validated on construction and on load; schema 18 |
| Primitive meshes | `crates/mesh/src/primitive.rs` (`build_primitive_mesh`), `crates/mesh/src/lib.rs` (`evaluate_primitive`, `primitive_mesh`, `EvaluationCache::object`), `crates/model/src/mesh.rs` (`Mesh::cube`) | derived from the parameters; the viewport and `object.collapse` share one evaluator; a cube keeps six faces |
| Primitive object card | `crates/ui/src/object_card.rs` (`ObjectCard::primitive_section`, `PrimitiveDraft`, `PrimitiveFields`) | the fields the shape has, as millimetres and whole numbers; one Apply is one `object.primitive.replace` |
| Contour edit commands | `crates/engine/src/panel_cmds.rs` (`PanelOutlineSet`, `PanelOutlineToContour`), `crates/engine/src/object_cmds.rs` (`ObjectContourReplace`) | small; `panel.outline.to_contour` bakes a rectangle outline into a ring (ADR 0061) |
| Contour panel card UI | `crates/ui/src/panel_card.rs` (`PanelAction::StartContour`, `PanelAction::ToContour`) | small; *Edit contour* bakes first when the panel is still rectangular |
| Contour object card UI | `crates/ui/src/object_card.rs` (`ObjectAction::StartContour`) | small |
| Contour segment value type | `crates/model/src/contour.rs` (`ContourSegment`, Line/Arc) | whole file |
| Contour geometry math | `crates/geom/src/contour.rs` (`offset`, `trimmed`, `intersections_with`, `sample`) | whole file |
| Planar regions and booleans | `crates/geom/src/region.rs` (`Region`, `Region::sample`, `union_regions`, `subtract_regions`, `intersect_regions`, `regions_from_rects`) | whole file |
| Panel core geometry | `crates/model/src/panel/geometry.rs` (`PanelGeometry::from_regions`), `crates/model/src/panel/machining.rs` (`core_violation_of`) | small |
| Machining descriptors | `crates/model/src/panel/machining.rs` (`MACHINING_DESCRIPTORS`, `MachiningDescriptor`, `MachiningCapabilities`) | small |
| Face triangulation | `crates/geom/src/triangulate.rs` (`triangulate_face`), `crates/mesh/src/lib.rs` (`object_mesh`) | whole files |
| Contour panel outline | `crates/model/src/panel/contour.rs` (`nominal_contour`, `contour_paths`, `to_contour_ring`, `BakedOutline`, `document_ring`) | whole file; a rectangle bakes into a ring and moves its banding (ADR 0061) |
| Panel commands | `crates/engine/src/panel_cmds.rs` (`panel.*`) | small |
| Model panel | `crates/model/src/panel/` (`Panel`, `geometry.rs`, `banding.rs`, `machining.rs`, `contour.rs`) | whole files |
| Model project | `crates/model/src/project/mod.rs` (`Project`, ids, revision, `scan_tree`) | whole file |
| Project envelope | `crates/model/src/project/wire.rs` (`ProjectWire`, `TryFrom`, migrations) | whole file |
| Project commands | `crates/model/src/project/{panel,object,node,camera_light,property}_cmds.rs` | whole files |
| Project queries and errors | `crates/model/src/project/query.rs`, `crates/model/src/project/error.rs` | whole files |
| Object commands | `crates/engine/src/object_cmds.rs` (`object.*`) | small |
| Solid plan and evaluation | `crates/param/src/solid.rs` (`SolidPlan`, `PlanBuilder`, `evaluate`), `crates/param/src/constructive.rs` (`build_constructive_mesh`) | whole files |
| Evaluation cache | `crates/mesh/src/lib.rs` (`EvaluationCache`, `NodeGeometry`, `evaluate_constructive`), `apps/furnicraft/src/scene_build.rs` (`evaluate`, `Evaluation`), `crates/engine/src/object_cmds.rs` (`OBJECT_COLLAPSE`) | one entry per node; a hit compares the derivation inputs, `finish` prunes unvisited nodes; ADR 0053 |
| Contour to profile conversion | `crates/param/src/profile.rs` (`region_of_contour`, `profile_of_region`, `profile_of_document_contour`, `fit_tolerance`) | whole file; the chord follows the shape and a bounded search coarsens it to fit one cap (ADR 0062) |
| Panel evaluation | `crates/param/src/lib.rs` (`build_panel`, `build_cutout_tool`, `build_panel_parts_with`, `validate_extrusion_contour`) | small |
| Machining pipeline | `crates/param/src/machining.rs` (`CompileMachining`, `OpEffect`, `PanelPlan`) | whole file |
| Scene file envelope | `crates/project/src/lib.rs` (`SceneFile`, `read`, `write`) | — |
| Panel card UI | `crates/ui/src/panel_card.rs` (`PanelCard`), `crates/ui/src/machining/` (`MachiningSection`) | — |
| UI scale | `crates/ui/src/ui_scale.rs` (`percent`, `factor`, `show`), `apps/furnicraft/src/main.rs` (`workspace`) | the control reads and writes `ctx.zoom_factor()` |
| View camera and projection | `crates/scene/src/lib.rs` (`Camera`, `Projection`, `toggle_projection`, `right`, `up`), `apps/furnicraft/src/main.rs` (`projection_button`, `viewport`, `viewport_owns_view`) | view state only; an authored camera pins navigation and the projection; the pitch reaches both poles, so top and bottom are perpendicular (ADR 0057) |
| Work plane and snapping | `apps/furnicraft/src/gizmo_math.rs` (`WorkPlane`, `from_camera`, `placement`, `snap_point`) | view state; the six standard ortho views select XY, XZ or YZ, every other view uses XY; snapping applies end, axis and grid in that order |
| UI icon resources | `assets/icons/`, `assets/README.md` (`Theme`, `Icon`, `icon_button`) | meaning in `assets/icons/STYLE_GUIDE.md` (§5 tone, §7.4 footprint, §9 axonometry), format and naming in `assets/README.md`, measurements and rejected options in `assets/icons/AUDIT.md`; the loader and button live in `crates/ui/src/icons.rs`, which also asserts the `3..21` footprint and 20–45 % coverage (ADR 0055) |
| Toolbar strip | `crates/ui/src/toolbars.rs` (`ToolPanel`, `Toolbars`, `ToolAction`, `ToolActivity`, `tool_activity`, `CONTOUR_TOOLS`, `extrude_hint`, `creation_chip`, `show`), `crates/ui/src/lib.rs` (`Editor::show_toolbars`, `creation_target`, `arm_creation`, `arm_primitive`, `extrude_path`, `create_panel` …) | one collapsible panel per theme; open state, the primitive size, the extrude height and the armed container are view state; every button is `icons::tool_button`; the armed drawing tool or primitive draws pressed; the only creation path, landing in the armed container or at the scene root (ADR 0056) |
| Path extrusion | `crates/ui/src/lib.rs` (`Editor::extrude_path`), `crates/engine/src/object_cmds.rs` (`ObjectExtrudeCreate`), `crates/model/src/contour.rs` (`Path::to_contour`) | a closed path becomes a sibling constructive body at the path's own placement and stays; an open path is refused before the command |
| Scene tree | `crates/ui/src/scene_tree.rs` (`show`, `Selection`, `TreeAction`, `AddChild`) | shows and restructures nodes, and arms a container to receive the next created node (U31); it creates nothing itself |
| Container card | `crates/ui/src/container_card.rs` (`ContainerCard`, `ContainerAction::AddChild`) | rename, role, and the *Add child* entry that arms the selected container (ADR 0056) |
| Plan gate | `state-contract/src/validate.rs` (`analyze`, `first_unfinished`, `calendar_date`), `state-contract/src/parse.rs` (`parse_backlog`), `state-contract/src/lib.rs` (`check_with`, `workspace_violations`) | one violation vector; every rule is a pure function in `validate.rs`, git facts come through `Git` |
