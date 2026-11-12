# 0094. A class registry gives the machine its semantics

- Status: Accepted; supersedes the registry section of 0035, refines 0034
- Date: 2026-11-12

## Context

`FurnType` (0035) was designed as a human classifier: six digits, a strict tree, a table in
`docs/domain/furntypes.md`. What it never became is something the program can use. The model
validates the *shape* of a code (a group is non-zero, a variant needs a type) but not its meaning, so
`20.01.09` is accepted although no such type exists. Every algorithm that needs to know what it is
looking at therefore still asks a name, a structural kind or a hand-written special case.

The mismatch is deeper than validation. The top level of the tree — `10` product, `20` part, `30`
facade, `40` hardware — classifies the *role an object plays in an assembly*, not what the object is,
while the second level classifies function. That is the defect 0035 itself charged the K3 classifier
with: two independent questions in one column. A shelf and a cabinet end up in different branches
because one is a part and the other a product, although both are storage.

What the program actually needs is the answer to three questions at once: *what is this*, *what may be
done with it*, and *where does it go in output*. A worktop proves the point. Physically it is a
board; in commerce it is linear stock bought by the metre; in the editor it must not stretch across
its width; on the shop floor it receives sink and hob cutouts and never enters a nesting plan. A
hinge proves the same the other way round: it is bought by the piece, carries a cup bore and a
mounting pattern, is never nested, never banded and never resized.

## Decision

**A semantic class replaces `FurnType`.** The document property key is `class`, the Rust value is
`Class` with a `ClassCode` inside it, and the word `FurnType` leaves the code, the docs and the UI.
The stored form stays six digits `GG.TT.VV` over three levels (group, type, variant) with `000000`
meaning unclassified — the width is proven, sorts, prefixes and costs nothing to migrate before the
first release.

**The code identifies; the class carries the rules.** A registry row is `code`, `parent`, `slug`,
`label_key`, `traits`, `since`, `retired`. The registry is a constant table in `furnicraft-model`,
validated by tests and checked against its document table; it is append-only. Inheritance is resolved
once at compile time, so a lookup is a bit test and nothing walks parents at runtime.

**The rules are a small, closed trait set.** Version 1 of the vocabulary:

| Trait | Values | What the machine may do with it |
|---|---|---|
| `supply` | `MadeFromSheet`, `MadeFromLinear`, `BuyPiece` | nesting versus linear cutting versus purchase |
| `measure` | `Area`, `Length`, `Count`, derived from `supply` unless overridden | the unit of a report line |
| `nesting` | `Member`, `Never`, derived from `supply` | whether a cutting plan owns it |
| `extent` | `Free`, `LengthOnly`, `None` | which handles a gizmo offers and which resize commands run |
| `operations` | `Holes`, `Grooves`, `Contour`, `EdgeBand`, `CupBore`, `MountPattern`, `Cutout` | which machining a class admits |
| `host` | a set of classes it may sit in or on | whether an insertion or a mount is legal |
| `level` | `Product`, `Component`, `Part`, `Fitting` | report grouping, never the component marker (0034 keeps that rule) |
| `opening` | `Closes`, `None` | whether the class closes an opening, with the gaps and reports that follow |

**Algorithms branch on traits, never on codes.** A comparison with a code literal outside a test is a
defect, reviewed as one. A trait is introduced only together with a consumer that reads it and a test
that proves the wrong class is refused, so the table cannot grow into speculation.

**A class earns its code for one of two reasons:** at least one rule differs, or a report, a drawing
or a CNC program must name it separately. Differing only in wording is one class with one label.

**The registry is the only assigner.** The inspector picks a class from the registry tree; the model
rejects an unknown code on assignment; loading keeps an unknown number and reports a diagnostic. The
document table is generated from the registry and a test compares them, so prose cannot drift.

**Body and class are different questions.** `Panel`, `Container` and `Object` describe the body a
node draws; the class says what that body means. A shelf and a side panel are both `Panel` bodies and
different classes; a panel and a worktop are the same body with opposite traits.

## Consequences

- Behaviour stops being inferred from names, kinds or geometry, and it arrives with the object instead
  of being re-implemented per feature. Reports, the coming cutting list and CNC rules read the same
  traits, so they cannot disagree about which panels are shelves.
- The registry becomes part of the published export contract (0095): a reader in any language sees
  `supply`/`measure`/`nesting` in the `classes` table and needs no knowledge of our numbers.
- `docs/domain/furntypes.md` is replaced by `docs/domain/classes.md`; `PROJECT.md`, the query API and
  the UI are renamed in the same change, and the schema is bumped because the stored property value
  changes its serde tag. The bump costs no migration (0096).
- The trait set is a promise: a later change to an existing class's traits is a behaviour change for
  saved projects and needs its own record, like a schema change.
- The three levels are the ceiling. A fourth level would be a new schema decision; the depth beyond a
  variant belongs to the catalogue article (a 35 mm cup, a damper, a brand), not to the class.
- A workshop that needs a type the registry lacks sees it as unclassified rather than smuggling it
  into a free-form string; the reserved `90..99` range in each group exists for local codes.

## Related

- [0034](0034-nodes-carry-typed-properties.md) — every node carries typed properties; `class` is the
  reserved semantic one, and the component marker stays "a container carries a class".
- [0035](0035-furniture-types-form-a-numbered-tree.md) — the encoding survives; its registry role and
  its top-level grouping do not.
- [0095](0095-the-order-package-is-a-published-snapshot.md) — the export ships the class table.
- [`docs/domain/classes.md`](../domain/classes.md) — the specification and the trait semantics.
