---
name: draw-icon
description: Create, edit or review a FurniCraft icon: a toolbar geometry icon or a service action (confirm, cancel, undo, file, warning). Use only when adding or changing an SVG under assets/icons.
license: MIT
compatibility: opencode
metadata:
  audience: agents
  workflow: ui-assets
---

# Draw a FurniCraft icon

Icons are assets, not code (`AGENTS.md` §4.12). This skill is the checklist for adding, editing or
reviewing one; it does not apply to code that merely displays an icon.

## Source of truth

| Document | Normative for |
|---|---|
| `assets/icons/STYLE_GUIDE.md` | meaning, composition, the three families, service grammar |
| `assets/README.md` | viewBox, allowed elements, naming, folder layout, theme table |

When they disagree, the guide wins on meaning and the README wins on format; fix the losing file in
the same change. Read only the sections you need — the guide is long.

## 1. Name the family before drawing

- `geometry` — a CAD operation: line, extrude, panel, hole, array. Follows the
  `OBJECT + ACTION METHOD + RESULT` formula (guide §2) and the 2D/3D rules (§8).
- `view` — navigation and projection: orbit, pan, frame all, ortho/perspective. Flat, monochrome,
  heavy.
- `service` — chrome: confirm, cancel, undo, save, warning. Follows guide §19, not §8–§10.

A command in a toolbar stays a geometry icon; a save action next to tools stays a service icon.

## 2. Place it

- Geometry and view icons live in the theme named after the toolbar or surface:
  `assets/icons/<theme>/<name>.svg`.
- Service icons live in `assets/icons/actions/<name>.svg`; the full name set is in guide §19.6.
- The file stem is the id: lowercase ASCII kebab-case, no prefix, no size suffix.
- If the file does not exist yet, create it empty: an empty file is the "not drawn yet" placeholder
  and the button falls back to its text label. Never leave a half-drawn file.
- Add or update the row in the README theme table in the same change.

## 3. Answer the pre-draw questions

For a geometry icon (guide §16):

```text
Object:      what is depicted?
Action:      what changes or is created?
Method:      which input elements explain the method?
Result:      which part carries the emphasis?
Unnecessary: what can be removed?
```

For a service icon add:

```text
Convention:  which standard glyph does the user already know for this verb,
             and which enclosure (circle, triangle) marks its status?
```

If an element cannot be labelled with its role (guide §3), remove it.

## 4. Draw inside the contract

- `viewBox="0 0 24 24"`, ink inside `2..22`; centre near `(12, 12)`; coordinates on half units.
- One ink: `stroke="#000"`, `fill="none"`, round caps and joins. A solid silhouette may use
  `fill="#000"`. No colour, no class, no CSS variable, no theme token.
- Weights: guide 1.5, main contour 1.6–1.8, result 2.2–2.6, service 2.0–2.4 or solid. Nothing below
  1.5 units, because the icon may be drawn at 16 px.
- Allowed elements: `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `g`,
  `transform`. No `defs`, `mask`, `clipPath`, `style`, filter, gradient, pattern, image, font or text.
- Gaps are real path breaks, never a background-coloured patch or a mask.
- 3D uses the canonical isometric axonometry of guide §9; a panel is a plate, not a cube.
- Similar actions share a pattern; opposite actions share a grammar and differ in the distinguishing
  element, not in style.

## 5. Service icons

- One glyph = one action, identical in menu, toolbar, dialog and shortcut reference.
- Bare glyph = action; enclosed glyph (circle, triangle) = status.
- At most one modifier mark (plus, minus, exclamation) in a consistent corner with a real 1.5-unit
  gap.
- Heavier and simpler than geometry icons; drawn for 16 px, not shrunk from a toolbar drawing.
- A destructive action keeps its label or confirmation; an icon never replaces it.

## 6. Verify

- the action is clear without text, at 16 px, in one ink;
- the result is the strongest element; auxiliary geometry does not compete;
- no decorative lines, no random detail, no white patch;
- the metaphor matches the operation, not a neighbouring command;
- a service icon uses the same glyph on every surface and distinguishes status by enclosure;
- the SVG parses and rasterises at the size the button draws.

## 7. Register and close

- The same commit adds the file, the README table row, the button that draws it and a test for the
  empty-file fallback; a rename also updates `crates/ui/src/icons.rs`.
- A new theme or a moved icon is an API change: update the README table and `plan/map.md`.
- Never copy an icon from another product; a third-party asset keeps its licence and gets a row in
  `ATTRIBUTION.md` (AGENTS §4.6).
- Run `cargo xtask ci` before finishing; icon-only changes still pass the plan and format gates.
