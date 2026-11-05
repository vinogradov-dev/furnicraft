# 0059. A selected line is a screen-space overlay

- Status: Accepted
- Date: 2026-11-05

## Context
A path, a drawn contour or a circle is an unlit line chain, and the renderer draws it as a
one-pixel `LineList`. A selection tint mixed into the chain colour moves each channel by less than
the eye can resolve, and wgpu cannot widen a line primitive, so the selected drawing looked exactly
like every other one. Doubling the pipeline or uploading a quad strip would put view feedback into
the renderer, which only draws what the scene frame describes.

The shell already paints view feedback over the viewport image in egui: the gizmos, the camera and
light glyphs, and the mesh sub-object markers. Selection is view state (ADR 0029), so the same
place can carry a visible line highlight.

## Decision
A selected line chain has two representations, one per job:

- the scene frame gives it one explicit `SELECTION_LINE_COLOR` instead of a mixed tint, so the
  uploaded one-pixel chain already reads as selected and the colour is testable without a GPU;
- the shell keeps the selected chains of the frame it just uploaded, after the drag preview, and
  paints them again over the viewport image as an egui overlay: a dark underlay of 3.5 pt followed
  by a 2.25 pt bright stroke;
- the overlay is projected by a pure `selection_outline` function from the stored points and world
  transforms, and cuts a chain where it falls behind the camera. It never touches the document or
  the GPU;
- the renderer keeps its one-pixel line pipeline and knows nothing about selection.

## Consequences
A selected drawing reads at a glance on a brown panel and on a light one, and the outline follows a
move in the same frame because it is taken from the previewed frame the renderer received. The cost
is one cloned list of selected chains per upload and one projection per frame, both bounded by the
selection rather than the document. Picking still ray-casts the one-pixel chain with its own radius,
so the thick stroke is feedback only. If the renderer later gains thick lines or a GPU selection
pass, this decision is the one to revisit.
