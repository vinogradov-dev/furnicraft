# 0018. Picking ray-casts the scene description

- Status: Accepted
- Date: 2026-10-25

## Context

The viewport must turn a click into a document `Id`. Two implementations are possible: render an
id buffer in a second GPU pass and read the pixel back, or intersect the scene's world-space
triangles on the CPU. A readback stalls the frame and needs an extra colour attachment and a second
shader; the scene description already carries every drawable with its document id and world
transform.

## Decision

- `furnicraft_scene` owns a validated `Ray` and `SceneFrame::pick`.
- `Camera::ray_ndc` turns normalized device coordinates into a world ray, with the aspect ratio
  supplied by the viewport.
- `pick` tests the triangles of every drawable in world space and returns the nearest document `Id`.
  Malformed vertices and indices are skipped, so hostile data cannot panic or index out of bounds.
- Selection highlight is a copy of the frame with the selected drawable tinted; the base frame keeps
  its material colours, so clearing the selection needs no rebuild.

## Consequences

- Picking is testable without a GPU or a window, and it returns document ids rather than mesh
  indices or pointers.
- A click costs a linear scan over the scene's triangles. That is the right trade for an editor with
  a few hundred panels; if scenes grow, the picker can gain an acceleration structure without
  changing its API or the render target setup.
- The highlight re-uploads the scene's vertex buffers when the selection changes. A later
  incremental colour update or a GPU id pass can replace either side without touching the document
  or the command layer.
