# Session

```text
task: U22
goal: The Primitives panel creates a cylinder or a sphere as well as a cube, each mesh built from the one edge length the panel already has.

steps:
  [ ] `Mesh::cylinder` and `Mesh::sphere` with capped sides and closed poles
  [ ] the `PrimitiveKind` variants and the command dispatch
  [ ] the three panel buttons and the pending placement for each
  [ ] tests for both meshes, the command and the panel
  [ ] the full gate and the closing commit

now: `crates/model/src/mesh.rs`, adding the two primitives
blocked: nothing
run: cargo test -p furnicraft-model
resume: add `Mesh::cylinder` after `Mesh::cube`
map: the mesh row in `plan/map.md`
decisions: the edge length is the full extent, so the cylinder's radius is half the edge and its height is the edge, and the sphere's radius is half; the cylinder uses 32 sides, a multiple of four so its silhouette has axis points; both meshes are flat-shaded polygon faces with no repeated vertex in a face, and every ring is counter-clockwise seen from outside
```
