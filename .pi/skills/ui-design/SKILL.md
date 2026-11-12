---
name: ui-design
description: "Design and review FurniCraft's interface: a panel, dialog, toolbar cell, ribbon, popover, command bar, palette, icon, colour, spacing, wording, cursor, drag or keyboard behaviour. Use before changing anything under crates/ui or the app chrome in apps/furnicraft/src, and when judging whether a screen works for a furniture maker."
license: MIT
compatibility: pi
metadata:
  audience: agents
  workflow: ui-design
---

# Design the FurniCraft interface

The user is a furniture maker, not a designer: they measure panels, fit hardware and drill holes.
The interface must feel like a native professional tool — dense, quiet, predictable — and it must
never make the person think about the program instead of the cabinet. This skill is the procedure
and the rubric for that; it is not a style essay and it does not replace an ADR.

## The bar is not a specification

"Make it like Blender" names a **quality bar**, never a design. The model knows a general idea of a
professional 3D editor and nothing about our decisions — which button, which modifier, what the
drag threshold is, what a grip means here. Any borrowed behaviour is decomposed before it is used:

```text
intent:      what the person is trying to achieve
input:       which button, modifier, click or drag, and what cancels it
feedback:    what changes on screen, in which order, within which budget
recovery:    what undo, Esc or a second click restores
edge:        the awkward case — no selection, two objects, a locked node
```

Four or five of these become our decision, written in our words, and recorded in an ADR when it is
not reversible. "As in X" on its own is refused: it is a guess wearing a citation.

## Sources of truth

Read the row and only the row you need; never re-derive a rule that is already written.

| What | Where |
|---|---|
| The rules of this project | `AGENTS.md` §4.1 (commands), §4.10 (space and views), §4.11 (assistant), §4.12 (icons) |
| The product and its vocabulary | `PROJECT.md`, `docs/domain/*.md` |
| Where a feature lives | `plan/map.md` (feature → file + anchor) |
| Tokens, palettes, metrics | `crates/ui/src/theme.rs` (`Palette`, `Ramp`, `ThemePreset`, `STEP`, `RADIUS`, `RIBBON_*`, `HEADER_*`, `STATUS_HEIGHT`, `contrast_ratio`) |
| Icon meaning and format | `assets/icons/STYLE_GUIDE.md`, `assets/README.md` |
| A user-facing word | `crates/text/src/messages.rs` (ADR 0086) |
| Why an earlier decision was made | `docs/decisions/`, one short file per decision |

When a document and the code disagree, the code is the evidence and the document is the defect:
fix the document in the same change (AGENTS §1, "if the hint is wrong, fix the hint").

## Invariants that are not a matter of taste

- Every document change is a command dispatched by id; the UI never writes a placement, a panel or
  a property (AGENTS §4.1, §4.10). A new surface that needs a mutation needs a command first.
- `ui` does not see `wgpu`; the shell wires the renderer. State that is not document data — camera,
  selection, hover, armed tool, open containers, layout — lives in the session or the shell (§4.10).
- Colours, radii, heights and gaps come from `theme.rs`; a literal colour in a feature module is a
  defect (ADR 0063, 0089). Hover, press, disabled and hairline states are mixed from the palette,
  never hand-picked.
- Numbers are typed through the one scrubbing field (`crates/ui/src/scrub.rs`, ADR 0092), with the
  unit outside the box; a bespoke drag arithmetic is a second field, and we have one.
- Every user-facing string is a message with a stable id (`crates/text`), not a literal: a missing
  translation must be a compile error, not an English word in a Russian screen.
- A floating surface is an `egui::Area` on `Order::Middle` over the scene and is declared to
  `Chrome::covers`, so a click on it is chrome and never a pick, a border drag or a camera gesture
  (ADR 0080). The scene rectangle does not shrink when a panel folds.
- Long work previews; it does not block. A changeable value fills `Session::preview` and one
  confirmed action commits it (ADR 0093).
- Nothing is available only by hovering, and nothing destructive is confirmed by a dialog when undo
  would do.

## The product in one screen

- Nodes: a container (component when it carries a furniture type), a panel with an outline and
  machining, a drawn path, a body grown from a path, a parametric primitive, a camera, a light.
- Every node has an origin, a pivot, a placement and a typed property bag; a panel keeps its frame
  at the minimum corner, faces `A`/`B`, vertices and edges with stable ids (ADR 0083).
- Two manipulation modes: move and rotate, both acting at `pivot_local` (ADR 0026, 0088).
- Creation is armed, not modal: a tool arms, the next work-plane click creates, Esc disarms,
  a preview shows the result before the commit.
- Surfaces: header and tabs, ribbon, scene tree and inspector panels (floating), view cluster,
  command bar, status bar, command palette, Settings, Home. Their files and ADRs are listed in
  `references/surfaces.md`.
- The person's work: overall size, panels and thickness, materials and grain, edge banding,
  hardware, holes and cutouts — millimetres, sheets, pieces.

## Procedure

1. **Brief in five lines** before any code: the screen and its owner, the person's goal, the one
   primary action, the density (toolbar / inspector / transient), and what is removed. A surface
   with two primary actions has not been briefed.
2. **Options**: two or three, each with its cost in code and in learned conventions. One option is
   not a design, three is a debate; write the trade-off in a line each.
3. **Choose against the rubric** (`references/checklist.md`), not against taste. Name the criteria
   you used; "looks cleaner" is not a criterion.
4. **Record** an ADR when the decision is not reversible (a new mode, a new gesture, a removed
   affordance, a changed default). A change that makes the user see something different out of the
   box keeps the old behaviour behind a setting, or the owner approves the break.
5. **Implement through the tokens and the shared widgets**, not beside them. If a widget is missing,
   the shared one grows; the feature does not grow a private copy.
6. **Verify**, then move on. The default check is headless; the eye check is named explicitly.

## Human factors

Load `references/human-factors.md` when the decision is about target size, distance, timing,
effort, memory or colour. It carries the numbers and the source for each, so the argument is not
"feels better".

## Verification

Silent by default, so a session never takes the user's screen:

- headless: an `egui::Context` driven with `RawInput` — rectangles, layer order, hit testing,
  texts, focus, the accessibility tree, and that no document revision moved;
- pixels (colour, contrast, translucency, the 55 % panel over the model, the light theme) are not
  covered headlessly: they need an offscreen render to PNG or a live run, and the task says which
  one was used.

A UI change is not done when it compiles. It is done when the result was looked at (AGENTS §4.7).

## Growth

The expert is only as good as its inventory. A change that adds a surface, a gesture or a shortcut
updates `references/surfaces.md` in the same change, exactly as `plan/map.md` is updated for a new
feature. Cite paths, constants and ADR numbers — never copy a colour, a size or a sentence into the
skill, or it will be wrong within a release and quietly outvote the code.

## Stop and ask the owner

- The domain: a new kind of object, a manufacturing rule, a field in the document (AGENTS §4.9).
- A new default the user will meet on first launch.
- A second way to do one thing, a new mode, or a gesture that collides with an existing one.
- Removing an affordance that exists today.

## References

- `references/human-factors.md` — Fitts, Hick, feedback budgets, target sizes, contrast, motor rules.
- `references/surfaces.md` — every surface, its file and its ADR.
- `references/checklist.md` — the review form and the eyes-only list.
