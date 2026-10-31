# Map

One row per feature: where it lives and the anchor to grep for. Before reading a large file, find
the feature here and read the range around its anchor, not the whole file. Update a row only when
the feature moves — a new module or a rename — not after every task. The dashboard prints this file
as a `MAP` block.
The plan gate does not check this file; a stale row is a defect the reader fixes.

| Feature | Where | Notes |
|---|---|---|
| Contour ring, segment ids | `crates/model/src/lib.rs` (`ContourRing`, `ContourSegmentId`, `assemble`) | ~4600 lines; search the anchor |
| Contour edit math | `crates/model/src/contour_edit.rs` (`move_corner`, `trim_segment`, `split_segment`) | whole file |
| Contour segment value type | `crates/model/src/contour.rs` (`ContourSegment`, Line/Arc) | whole file |
| Contour geometry math | `crates/geom/src/contour.rs` (`offset`, `trimmed`, `intersections_with`, `sample`) | whole file |
| Contour panel outline | `crates/model/src/panel_contour.rs` (`nominal_contour`, `contour_paths`) | whole file |
| Panel commands | `crates/engine/src/panel_cmds.rs` (`panel.*`) | small |
| Object commands | `crates/engine/src/object_cmds.rs` (`object.*`) | small |
| Scene file envelope | `crates/project/src/lib.rs` (`SceneFile`, `read`, `write`) | — |
| Panel card UI | `crates/ui/src/panel_card.rs` (`PanelCard`) | — |
