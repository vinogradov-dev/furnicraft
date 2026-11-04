# 0053. The evaluation cache is keyed by derivation inputs, not by the revision alone

- Status: Accepted
- Date: 2026-11-04

## Context

`furnicraft-mesh` had no cache: every scene rebuild walked the whole tree, built every panel body,
every band strip and every constructive object again, then tessellated them all. A one-panel move
re-tessellated a whole cabinet, and a camera orbit — which rebuilds nothing — still paid for the
first edit after it.

The document has one global `Revision`, and `docs/domain/geometry.md` promised a cache keyed by
`(Revision, Id)`. That key cannot be right on its own: every command bumps the same counter, so
`(revision, id)` misses for every node on every edit and the cache would never hit. The revision
answers "is this value newer than the document", not "did this node's geometry change".

## Decision

- `EvaluationCache` keeps one entry per node, keyed by `Id`, holding the derived `NodeGeometry`
  (shared `Arc` meshes), the revision that built it, and the **derivation inputs** it was built
  from.
- A panel's inputs are its outline, nominal size, machining list, edge, corner and ring banding,
  band order, resolved thickness and catalogue-resolved band profiles. An object's input is its
  whole geometry payload. A hit compares those values; a node whose inputs hold reuses its mesh
  even at a new revision.
- A pass is bracketed by `start` and `finish`. `finish` drops every entry the walk did not visit,
  so a deleted node releases its mesh and a stale entry cannot outlive its node.
- The cache is the single evaluator: `scene_build` reads a panel, an object body and a path through
  it, and `object.collapse` bakes with the same `evaluate_constructive`, so the collapsed mesh is
  exactly what the viewport drew.
- The cache is plain data in `crates/mesh` (L3). It owns no kernel body and no GPU resource; every
  body a miss builds is released before the call returns.

## Consequences

- A move, a rename, a selection or an edit to another node costs no tessellation. That is the
  interaction quality the Blender bar asks for.
- The key lists every input by hand, so a new derived feature must add its parameter to the input
  set or the cache will serve a stale mesh. A test per family guards the common cases; a future
  feature has to extend the list.
- Comparing inputs costs a clone of the panel's machining and banding on a miss and a comparison on
  a hit. On a large cabinet this is far below one kernel call; if it ever shows up, a revision
  stamp per node would replace the comparison, which needs a document field and is deliberately not
  taken yet.
- Recording the revision a hit kept makes the reuse observable: `built_at` shows that an unchanged
  node was not rebuilt.
