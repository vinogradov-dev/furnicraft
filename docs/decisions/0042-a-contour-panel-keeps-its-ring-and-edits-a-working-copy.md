# 0042. A contour panel keeps its ring and edits a working copy

- Status: Accepted
- Date: 2026-11-02

## Context

A panel could only be the nominal rectangle shaped by corner features and bows (ADR 0009). That
describes a board whose corners are cut and whose edges bow, but not a part that is cut to an
arbitrary outline — an L-shaped top, a sink cut-out profile, a rounded counter front. The shape
already exists as a constructive contour (ADR 0040), yet a panel stores nominal length and width and
re-derives a rectangle, so the only way to keep an arbitrary outline would be to turn every such
panel into an object and lose the panel rules: material thickness, banding, machining and the
cutting-list identity.

Editing that outline also raised a separate question. Reshaping a ring means many intermediate
corners, each of which may be invalid, and a command per drag would flood the undo history and let
the document hold a half-drawn shape.

## Decision

- A panel's outline is a tagged value: `Rect`, the nominal rectangle with its optional corner
  features and bows, or `Contour { ring }`, a stored `Contour` ring. The variant is chosen at
  creation and never changes. A rectangle panel never converts into a contour panel; a new panel is
  created from the ring instead.
- `panel.contour.create` copies the ring of a constructive object into a new contour panel, moves it
  so its minimum corner is the panel-local zero (ADR 0009) and derives the nominal size from its
  bounds. Segment ids travel with their curves, so banding named by a segment keeps its meaning.
- The ring *is* the outline: a contour panel stores no corner features and no bows, and its body is
  the ring extruded from `z = 0` to the material thickness. Its sides are ring segments, not
  rectangle edges 1–4, and banding is one record per `ContourSegmentId` (`RingBanding`,
  `SegmentSource::Ring`) with the same material, width and `included_in_size` rules as an edge band.
- An outline edit may move the ring's minimum corner. `panel.outline.set` re-anchors the ring and
  moves the panel placement, pivot and machining by the inverse offset, so the world-space shape and
  every feature stay where the user drew them. A ring that no longer spans a usable panel size, or an
  edit that drops a segment carrying banding, is refused before the document changes.
- Contour editing is view state (ADR 0029). The editor keeps the stored ring as `base` and a `draft`
  working copy; the viewport gizmo only rewrites the draft, and the document, its revision and the
  undo history stay untouched until the user applies.
- The mode is entered on exactly one selected node that stores a ring: a contour panel outline or a
  constructive object contour. A rectangle panel, a mesh object and a multi-selection have nothing to
  edit. Mesh editing and contour editing are mutually exclusive.
- The draft supports moving a corner and splitting a side. An arc corner slides along its own circle:
  an edit trims or extends the sweep and never redefines the centre or radius. Untouched segments keep
  their ids, a split keeps the first id and mints the new part from the ring counter, and every
  intermediate draft is validated as a ring, so a point that would open or cross the ring leaves the
  last valid shape in place.
- Apply dispatches exactly one command — `panel.outline.set` for a panel, `object.contour.replace`
  for an object — so the whole edit is one undo entry. Cancel drops the draft and changes nothing. A
  rejected apply keeps the draft so the user can fix it or cancel. An untouched draft dispatches no
  command. A selection change or a removed target leaves the mode.

## Consequences

- Panels and constructive objects share one ring representation, one edit math and one set of segment
  ids; a band, a selection or a future operation can name a segment in either.
- The panel card and the commands ask the outline for its segment sources instead of branching on a
  node kind. Rectangle-only controls (length and width fields, corners, bows, edge banding) are hidden
  for a contour panel, and ring-only controls are hidden for a rectangle.
- A contour panel is sized by its ring, so `panel.resize` is refused; the way to a different size is
  another outline edit or a new panel.
- Re-anchoring on an edit keeps the panel frame at the outline's minimum corner but shifts machining
  coordinates with it. A future operation that stores coordinates in the ring frame must follow the
  same rule.
- The core trace and the cutout fit still use the bounds rectangle of the ring. A ring whose material
  does not fill those bounds is traced as if it did until the region-based core trace lands; this is
  recorded as a gap rather than encoded as an invariant.

## Related

- [`docs/domain/panels.md`](../domain/panels.md) — the contour panel, ring banding and the commands.
- [`docs/domain/geometry.md`](../domain/geometry.md) — the contour ring and the edit mode.
- [0009](0009-contour-first-panel-model.md) — the nominal frame and the contour-first panel model.
- [0040](0040-a-constructive-contour-is-a-ring-of-segments.md) — the ring and its stable segment ids.
- [0029](0029-view-state-belongs-to-the-session.md) — why editing state is not document data.
