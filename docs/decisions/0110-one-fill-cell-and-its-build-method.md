# 0110. One fill cell, and its build method in the cell's own list

- Status: Accepted; refines 0106 and the ribbon's method list (U74)
- Date: 2026-11-14

## Context

The empty-space tool has two cells — Post and Shelf — and each offers two ways to build the part:
the largest rectangle that fits the niche, or the niche's whole shape. The owner reports that the
second row reads as a second panel: it has no artwork, and after picking it the cell goes on drawing
the pinned fit, so the choice looks like it did not take. He asks for one panel, with the way it is
built moved into a secondary menu and a visible selected state.

## Decision

**One cell per panel kind.** A post and a shelf are different parts and keep their own cells; the
build method is not a third part and gets no cell of its own.

**The build method is the cell's method list, captioned.** Both rows sit under a «Способ построения» /
“Build method” caption, so the list reads as two builds of the panel its header names. The shape row
carries the polygon glyph; no row is a blank line.

**The cell always draws the panel kind.** The main face keeps the post or shelf glyph while a mode is
armed or pinned, so changing the build never turns the cell into another part.

**The armed mode is what the cell shows.** While a fill is armed, the face draws the armed method,
pressed, with that method's hint; its click runs the armed method, which is the second click that
drops the tool. When no fill is armed the face draws the pinned method, which stays the workspace
default and the row the list marks. Picking a row therefore changes the picture at once without
silently rewriting the saved default; the pin is still the explicit way to change it.

**The rows are named for what they build.** “Largest rectangle” / «Наибольший прямоугольник» and
“Niche shape” / «По форме ниши». The hint keeps the tool's name in front — «Стойка: По форме ниши» —
because two cells would otherwise read the same.

## Consequences

- Choosing the shape is visible on the cell and reversible with the same click; the mode is no longer
  a row that looks disabled because the cell above it did not change.
- The list loses no affordance and the ribbon gains no second primary action. The pin still marks and
  changes the saved default; running a row does not write the workspace file.
- A fill cell can now be pressed while the list's accent sits on another row. The accent says what
  the next plain click will run; the pressed cell says what this click armed. They are different
  questions and the picture answers both.
- `docs/domain/products.md` §7 names the two modes by their new words. No new surface, gesture or
  shortcut is added, so the UI skill's surface inventory does not change.

## Related

- [0106](0106-a-niche-is-a-convex-cell-of-wall-planes.md) — the niche the modes read.
- [0108](0108-a-shaped-fill-bevels-its-ends-onto-the-walls.md) — the shape mode's end bevel.
- [0109](0109-a-niche-is-bounded-by-the-faces-the-pointer-sees.md) — the pointer the fill measures.
- [`crates/ui/src/toolbars/spec.rs`](../../crates/ui/src/toolbars/spec.rs) — the two fill cells and
  their variant lists.
