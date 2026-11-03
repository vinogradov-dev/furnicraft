# Map

One row per feature: where it lives and the anchor to grep for. Before reading a large file, find
the feature here and read the range around its anchor, not the whole file. Update a row only when
the feature moves — a new module or a rename — not after every task. The dashboard prints this file
as a `MAP` block.
The plan gate does not check this file; a stale row is a defect the reader fixes.

| Feature | Where | Notes |
|---|---|---|
| Contour ring, segment ids | `crates/model/src/contour.rs` (`Contour`, `ContourSegmentId`, `assemble`) | whole file |
| Contour edit math | `crates/model/src/contour_edit.rs` (`move_corner`, `trim_segment`, `split_segment`) | whole file |
| Contour edit mode | `crates/ui/src/contour_edit.rs` (`ContourEdit`), `crates/ui/src/lib.rs` (`Editor`), `apps/furnicraft/src/contour_gizmo.rs` (`ContourGizmo`), `apps/furnicraft/src/main.rs` | whole files |
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
