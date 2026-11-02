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
| Face triangulation | `crates/geom/src/triangulate.rs` (`triangulate_face`), `crates/mesh/src/lib.rs` (`object_mesh`) | whole files |
| Contour panel outline | `crates/model/src/panel/contour.rs` (`nominal_contour`, `contour_paths`) | whole file |
| Panel commands | `crates/engine/src/panel_cmds.rs` (`panel.*`) | small |
| Object commands | `crates/engine/src/object_cmds.rs` (`object.*`) | small |
| Constructive evaluation | `crates/param/src/lib.rs` (`build_constructive_mesh`, `validate_extrusion_contour`) | small |
| Scene file envelope | `crates/project/src/lib.rs` (`SceneFile`, `read`, `write`) | — |
| Panel card UI | `crates/ui/src/panel_card.rs` (`PanelCard`) | — |
