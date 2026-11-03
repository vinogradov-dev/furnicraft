# 0046. Machining compiles to staged effects through one descriptor table

- Status: Accepted
- Date: 2026-11-03

## Context

With anchored machining records in place (ADR 0045), the only operation was still the cutout, and
every consumer matched on it: the panel builder skipped records that were not cutouts, the load path
checked that a cutout had a face anchor, and the add command did the same. The next operations —
grooves, milling, banding — would each have to repeat that shape in the builder, in validation and in
the card, and a rule could drift between them.

The card also has to offer the operations without knowing how they are built, and `ui` may not depend
on `param`. The facts the card needs are static: what kinds exist, how they group, when they apply,
which anchors and depth modes they accept, and what parameters a new one starts with.

## Decision

The static facts live in the document; the geometry lives in `param`.

- `furnicraft-model` owns one static descriptor table, `MACHINING_DESCRIPTORS`, with one
  `MachiningDescriptor` per `MachiningKind`: kind, add-menu category, pipeline stage, capabilities
  and a default-parameters constructor. `MachiningParams::descriptor()` maps a value to its row.
  The table is not serialized: the file stores the kind tag and the build maps it back.
- Capabilities are the rule the load path and the add command share: an operation that declares no
  edge anchor cannot be stored on one. `PanelError` and `ProjectError` name the refusal
  `MachiningAnchorUnsupported` instead of the cutout-specific `MachiningNeedsFace`.
- `furnicraft-param` owns the compiler. `CompileMachining` matches a parameter kind exactly once and
  returns `CompiledMachining { stage, effects }`. `OpEffect` is `CutRegion(Region)`,
  `Subtract(SolidPlan)`, `Add(SolidPlan)` or `Attachment(EdgeRef)`.
- `PanelPlan` folds the effects: a cut region subtracts from the core before extrusion, a subtract or
  add appends a boolean step in list order, and an attachment is kept for the banding path without
  touching the solid. The fold keeps those parts apart, so a shape effect still applies before the
  extrusion even when its record comes later in the list.
- The stages are `Shape` (changes the core region), `Post` (a boolean on the extruded body) and
  `Deform` (reserved for the deferred bend). The cutout is `Post`. A record at the deform stage is
  refused by the fold for now, rather than applied in the wrong order.

## Consequences

- The panel builder contains no kind match: `build_panel` is `PanelPlan::of_panel(...).solid()` and
  the geometry kernel only sees the folded plan.
- A new operation adds a `MachiningParams` variant, a descriptor row and one compiler arm. The
  builder, the fold, the record and the commands do not change.
- The card builds its add menu from the table and reads capabilities to show only the controls a
  kind offers (D7). It still cannot depend on `param`.
- The default operation is a starting value, not a domain rule: the descriptor has no panel to
  measure, so the add command validates it against the actual core and may reject it.
- `Add` and `Attachment` have no producer yet; they exist because the fold and the compiler must
  describe what a later bandwidth or hardware operation does without changing the pipeline.

## Related

- [`crates/model/src/panel/machining.rs`](../../crates/model/src/panel/machining.rs) — records, descriptors, capabilities.
- [`crates/param/src/machining.rs`](../../crates/param/src/machining.rs) — the compiler, effects and the panel plan.
- [`crates/param/src/solid.rs`](../../crates/param/src/solid.rs) — the plan the fold produces.
- [0045](0045-machining-is-an-ordered-list-of-anchored-operations.md) — the record shape.
- [0041](0041-every-solid-compiles-to-one-solid-plan.md) — one solid plan per body.
- [0044](0044-one-outline-source-resolves-the-panel-core.md) — the core region the plan starts from.
