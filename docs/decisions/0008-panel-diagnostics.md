# 0008. Panel diagnostics and command invalidation

- Status: Accepted
- Date: 2026-10-23

## Context

A panel can be wrong without being unreadable: its material can disappear, banding can lose its
profile, a legacy file can lack fallbacks, or machining can stop fitting after a core change. A
command can also be asked for an impossible size. Treating every problem as a load failure loses the
user's work; accepting every command silently hides a broken model.

## Decision

Validation has two outcomes. Command errors are for invalid inputs and impossible geometry: unknown
ids, unknown catalogue materials, non-positive nominal/core size, and a resize that would leave a
machining feature outside the core. A failed command leaves the document unchanged.

Derived diagnostics are for states the document can keep: `NoMaterial`, `MissingMaterial(MaterialId)`,
`MissingBandingMaterial { edge, material }`, `MissingBandingProfile { edge }`, `NonPositiveCore`,
`MachiningOutsideCore`, and `BlindDepthNotLessThanThickness`. They are computed during resolution and
are not stored in the document or the scene file.

The document's hard invariants are identity, valid nominal dimensions, finite values, unique banding
edges and valid fallback dimensions. Core positivity and machining fit are diagnostics, not
deserialization errors: a catalogue change must not make a saved scene unreadable. The command layer
still applies the stricter resize rule above.

`panel.resize` rejects when existing machining no longer fits. `panel.material.set` and
`panel.edge_banding.set` reject an unknown material id and a non-positive resulting core, but accept a
valid change that makes machining invalid; the panel keeps its data, is marked as a problem panel and
is drawn where possible. A missing catalogue entry is never substituted or deleted.
`panel.edge_banding.clear` only adds room and cannot invalidate machining.

A problem panel is still selectable and editable; the diagnostic disappears when the cause is fixed.
Rendering uses a neutral body colour for a missing material, skips a band with no usable profile, and
never turns a diagnostic into a document mutation. Undo snapshots contain the document only; the
catalogue and diagnostics are outside the snapshot.

## Consequences

- A user can keep working with a broken catalogue and repair it later.
- Resizing is stricter than material selection, because it is an explicit size edit.
- Diagnostics must be recomputed after every load, catalogue change and geometry rebuild.
- The UI needs a place to show issues in M5; the model and scene do not store them.
- Legacy data without a banding fallback loses the band visually but not the band record.
