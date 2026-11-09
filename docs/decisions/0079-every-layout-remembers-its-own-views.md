# 0079. Every layout remembers its own views

- Status: Accepted
- Date: 2026-11-09

## Context

The workspace offered one, two or four views through a single tree that `set_kind` reshaped. Moving
from four views to one truncated the extra leaves, so the cameras, presets and split ratios a user had
arranged in the four-view layout were gone when they came back. The tree was therefore a mode switch,
not a remembered workspace.

The pointer named the target at the same time: orbit, pan, zoom and the cluster's *Home* acted on the
view under the pointer, while `O` acted on the active view. Two rules for the same gesture made the
active frame and the drawn controls disagree, and a wheel scroll over a neighbour silently moved a
view the user had not selected.

## Decision

- `Layout` holds one `LayoutState` per `LayoutKind`. A state owns its tree, its leaves, its active
  leaf and its next free leaf id. `set_kind` only chooses which state is shown; it never reshapes,
  truncates or copies a tree.
- Every layout's state is built with the workspace from the fit camera and that layout's standard
  presets: `Quad` is top-left *Front*, bottom-left *Top*, top-right *Left*, bottom-right
  *Axonometric*; `TwoColumns` and `TwoRows` are *Front* then *Axonometric*; `Single` follows the
  settings opening view. A standard preset is orthographic by the existing rule (ADR 0049, U59).
- `reset_cameras(camera, opening)` replaces the camera of every stored layout and reapplies that
  layout's standard presets. The trees, ratios and active leaves stay. Opening a file therefore keeps
  the split the user arranged but never leaves a view looking at where the old model was.
- The active view is the only target. Orbit, pan, zoom, `O`, *Home*, the presets, selection, the
  gizmos and the cluster all act on `Layout::active()`. A press of any pointer button over a view
  makes it active; a primary press on a border starts a resize and changes no active view. The wheel
  zooms only while the pointer is over the active view.
- `Layout::press` owns the press rule, so the shell and the tests read the same branch instead of
  repeating it.

## Consequences

- Nine cameras live in the shell (1 + 2 + 2 + 4), which is negligible and never reaches the
  document; the layout states are view state like the cameras they hold (ADR 0029).
- Switching layouts is instant and lossless; the cost is that the standard presets are reapplied on
  opening a file, so a custom preset a user had picked in a view is reset to that layout's standard
  one. Ratios and active views are not.
- A gesture can no longer reach a neighbour without a press, so the frame the user sees as active is
  always the frame the gesture moves.
- The cluster's *Home* and the keyboard no longer need to know where the pointer is; the shell's
  `pointer_leaf` helper and the `target_leaf = hovered` fallback are gone.
