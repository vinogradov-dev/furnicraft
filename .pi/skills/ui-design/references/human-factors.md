# Human factors

The numbers behind a design argument. Each rule names its source so the next session does not have
to trust a feeling. Values are the floor for a desktop tool at 100 % scale; they scale with
`ui_scale`.

## Motor: reaching and hitting

- **Target size**: 24 × 24 px minimum for any clickable control (WCAG 2.2, 2.5.8). In the ribbon,
  a cell is not a button: the whole `theme::RIBBON_CELL_WIDTH` × `RIBBON_ROW_HEIGHT` area is the
  target, and only the drawing is smaller.
- **Near misses count**: a target carries 2–4 pt of slack around its visual shape; a 16 pt icon in a
  24 pt box, not a 16 pt click area. The gizmo already works this way — the ring hit radius in
  `apps/furnicraft/src/rotate_gizmo.rs` is wider than the stroke it draws.
- **Fitts (1954)**: time to a target grows with distance and shrinks with width. Consequences: the
  most frequent action sits closest to where the hand already is (the cell under the pointer, the
  view's own corner), and edge-anchored targets (a status bar row, a panel header) are "infinitely"
  wide in one direction — use that instead of making a box bigger.
- **Click or drag**: disambiguate by a threshold (3–5 pt), never by time. A press that moves less
  than the threshold is a click and must not move the object; once a drag starts it captures the
  pointer, keeps working when the pointer leaves the widget, and ends on release anywhere.
- **No hidden verbs**: nothing happens only on hover, only on double-click, or only with a modifier
  the tooltip omits. A drag handle that is invisible until hover is allowed only when the same
  action exists in a menu or by keyboard.
- **Cancel is a place**: Esc and a second click restore the previous state of every armed tool,
  running draft and previewing command (AGENTS §4.10, ADR 0052, 0093).

## Perception: seeing reaction

- **Feedback budgets** (Nielsen, response times): under 0.1 s reads as instant; under 1 s keeps the
  train of thought; 10 s is the limit of attention. Our frame budget and the 250 ms rebuild notice
  (`Document::busy`, `SLOW_REBUILD`) exist for the middle band: past it the window says so before
  it blocks.
- **The hook must move first**: hover, press and armed state change on the press frame, before the
  geometry solves. A control that changes only after a rebuild feels broken.
- **Never a dead frame**: a long operation shows progress where the eye already is (the status bar,
  the command bar), not in a new dialog.
- **Meaning never rides on colour alone** (WCAG 1.4.1): a selected row also has a bar, a shape or a
  weight change; an error also has text or an icon.
- **Contrast** (WCAG 2.2, 1.4.3 and 1.4.11): 4.5:1 for body text, 3:1 for large text, icons and
  control boundaries. `theme::contrast_ratio` is the measuring stick; `ACCENT_CONTRAST` is the floor
  the accent override is pushed to, and the same floor applies to a new token in both variants of
  every preset (ADR 0089).
- **Translucency**: a floating panel keeps its own surface legible over the brightest and the
  darkest thing in the scene. The panel opacity range is
  [`MIN_PANEL_OPACITY`](../../../crates/ui/src/settings.rs) — test at its two ends, never at 100 %
  only.

## Cognition: deciding and remembering

- **Hick (1952)**: choice time grows with the logarithm of equally weighted options. A menu of five
  frequent tools beats a menu of twenty; the rare ones belong in the command palette, not in the
  ribbon.
- **Recognition over recall** (Nielsen #6): the status bar names the armed tool and its gesture; the
  spine of the inspector names the selected node; a field shows its unit. The person should never
  have to remember what mode they are in.
- **Consistency over cleverness** (Nielsen #4): the same action has one icon, one label, one
  shortcut and one place, on every surface. A second location is a second design.
- **Progressive disclosure**: the folded panel, the collapsed section and the palette exist so the
  first screen stays quiet. Hiding is allowed only when the thing is also findable; a hidden
  control with no path to it is gone.
- **Error prevention beats error messages** (Nielsen #5): a refused value is clamped or disabled
  with a reason, a destructive action asks once and says what it destroys, and everything else is
  undo (AGENTS §4.1, the history is the safety net).
- **Grouping** (Gestalt proximity): 4 pt (`theme::STEP`) separates related controls, 8–12 pt
  separates groups; a caption belongs to its group, not to the screen.

## Text

- One term per idea, taken from `PROJECT.md` and the domain documents; a synonym is a bug.
- Sentence case for messages, the glossary's word for every noun; a command label is the verb the
  person says out loud ("Extrude", not "Extrusion operation").
- A field shows the unit outside the box (ADR 0092); a number is never naked.
- A message is a full sentence a person can act on: what happened, and what to do next.

## 3D-specific

- A gizmo keeps a constant screen size; it must not become a speck at distance or a wall up close.
- Camera conventions are fixed and documented (ADR 0057, 0078): the preset names, the pole, the
  roll, `O` for the projection. A new gesture must not shadow one of them.
- Snapping announces itself: the candidate point is drawn before the click, and the threshold is
  visible as a highlight, not as a rule in the manual.
- Picking returns document `Id`s and never a mesh index (AGENTS §4.10); an unfindable click selects
  nothing and says nothing.
