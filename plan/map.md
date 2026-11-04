# Map

One row per feature: where it lives and the anchor to grep for. Before reading a large file, find
the feature here and read the range around its anchor, not the whole file. Update a row only when
the feature moves — a new module or a rename — not after every task. The dashboard prints this file
as a `MAP` block.
The plan gate does not check this file; a stale row is a defect the reader fixes.

| Feature | Where | Notes |
|---|---|---|
| Contour ring, segment ids | `crates/model/src/contour.rs` (`Contour`, `ContourSegmentId`, `assemble`) | whole file |
| Flat path value | `crates/model/src/contour.rs` (`Path`, `PathError`, `to_contour`) | open or closed chain; `to_contour` applies the ring rules and keeps the ids |
| Path line primitive | `crates/scene/src/lib.rs` (`Polyline`, `SceneFrame::push_polyline`, `SceneFrame::pick`), `crates/render/src/lib.rs` (`polyline_vertices`), `apps/furnicraft/src/scene_build.rs` (`path_points`) | unlit chain; the renderer's line pipeline; a pick hits a segment within `LINE_PICK_RADIUS_POINTS` |
| Contour edit math | `crates/model/src/contour_edit.rs` (`move_corner`, `trim_segment`, `split_segment`) | whole file |
| Contour edit mode | `crates/ui/src/contour_edit.rs` (`ContourEdit`), `crates/ui/src/lib.rs` (`Editor`), `apps/furnicraft/src/contour_gizmo.rs` (`ContourGizmo`), `apps/furnicraft/src/main.rs` | whole files |
| Drawing draft | `crates/ui/src/contour_edit.rs` (`DrawDraft`, `DrawDraftError`), `crates/ui/src/lib.rs` (`Editor::start_drawing`, `drawing_point`, `undo_drawing`, `close_drawing`) | view state only; U17 commits it |
| Contour edit commands | `crates/engine/src/panel_cmds.rs` (`PanelOutlineSet`), `crates/engine/src/object_cmds.rs` (`ObjectContourReplace`) | small |
| Contour panel card UI | `crates/ui/src/panel_card.rs` (`PanelAction::StartContour`) | small |
| Contour object card UI | `crates/ui/src/object_card.rs` (`ObjectAction::StartContour`) | small |
| Contour segment value type | `crates/model/src/contour.rs` (`ContourSegment`, Line/Arc) | whole file |
| Contour geometry math | `crates/geom/src/contour.rs` (`offset`, `trimmed`, `intersections_with`, `sample`) | whole file |
| Planar regions and booleans | `crates/geom/src/region.rs` (`Region`, `Region::sample`, `union_regions`, `subtract_regions`, `intersect_regions`, `regions_from_rects`) | whole file |
| Panel core geometry | `crates/model/src/panel/geometry.rs` (`PanelGeometry::from_regions`), `crates/model/src/panel/machining.rs` (`core_violation_of`) | small |
| Machining descriptors | `crates/model/src/panel/machining.rs` (`MACHINING_DESCRIPTORS`, `MachiningDescriptor`, `MachiningCapabilities`) | small |
| Face triangulation | `crates/geom/src/triangulate.rs` (`triangulate_face`), `crates/mesh/src/lib.rs` (`object_mesh`) | whole files |
| Contour panel outline | `crates/model/src/panel/contour.rs` (`nominal_contour`, `contour_paths`) | whole file |
| Panel commands | `crates/engine/src/panel_cmds.rs` (`panel.*`) | small |
| Model panel | `crates/model/src/panel/` (`Panel`, `geometry.rs`, `banding.rs`, `machining.rs`, `contour.rs`) | whole files |
| Model project | `crates/model/src/project/mod.rs` (`Project`, ids, revision, `scan_tree`) | whole file |
| Project envelope | `crates/model/src/project/wire.rs` (`ProjectWire`, `TryFrom`, migrations) | whole file |
| Project commands | `crates/model/src/project/{panel,object,node,camera_light,property}_cmds.rs` | whole files |
| Project queries and errors | `crates/model/src/project/query.rs`, `crates/model/src/project/error.rs` | whole files |
| Object commands | `crates/engine/src/object_cmds.rs` (`object.*`) | small |
| Solid plan and evaluation | `crates/param/src/solid.rs` (`SolidPlan`, `PlanBuilder`, `evaluate`), `crates/param/src/constructive.rs` (`build_constructive_mesh`) | whole files |
| Contour to profile conversion | `crates/param/src/profile.rs` (`region_of_contour`, `profile_of_region`, `profile_of_document_contour`) | whole file |
| Panel evaluation | `crates/param/src/lib.rs` (`build_panel`, `build_cutout_tool`, `build_panel_parts_with`, `validate_extrusion_contour`) | small |
| Machining pipeline | `crates/param/src/machining.rs` (`CompileMachining`, `OpEffect`, `PanelPlan`) | whole file |
| Scene file envelope | `crates/project/src/lib.rs` (`SceneFile`, `read`, `write`) | — |
| Panel card UI | `crates/ui/src/panel_card.rs` (`PanelCard`), `crates/ui/src/machining/` (`MachiningSection`) | — |
| UI scale | `crates/ui/src/ui_scale.rs` (`percent`, `factor`, `show`), `apps/furnicraft/src/main.rs` (`workspace`) | the control reads and writes `ctx.zoom_factor()` |
| View camera and projection | `crates/scene/src/lib.rs` (`Camera`, `Projection`, `toggle_projection`), `apps/furnicraft/src/main.rs` (`viewport`, `viewport_owns_view`) | view state only; an authored camera pins navigation and the projection |
| Work plane and snapping | `apps/furnicraft/src/gizmo_math.rs` (`WorkPlane`, `from_camera`, `pointer_point`, `snap_point`) | view state; the six standard ortho views select XY, XZ or YZ, every other view uses XY; snapping applies end, axis and grid in that order |
| UI icon resources | `assets/icons/`, `assets/README.md` (`Theme`, `Icon`, `icon_button`) | meaning in `assets/icons/STYLE_GUIDE.md`, format and naming in `assets/README.md`; the loader and button live in `crates/ui/src/icons.rs` |
