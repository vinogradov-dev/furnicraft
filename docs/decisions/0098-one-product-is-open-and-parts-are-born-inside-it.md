# 0098. One product is open, and furniture parts are born inside it

- Status: Accepted
- Date: 2026-11-13

## Context

A furniture maker does not build parts in the void: a shelf is a shelf *of this cabinet*. The editor
today lets a panel be created at the root as easily as inside a group, so a scene can hold parts that
belong to nothing, and a report cannot tell whether a stray panel is work in progress or trash.

The owner's rule is direct: a furniture part is created only inside an **open** product, exactly one
product is open at a time, and a closed product accepts no new parts.

## Decision

**Openness is session state.** `Session` holds `active_product: Option<Id>`. It is never serialised:
the document has no "open" field (ADR 0029). Opening a product replaces the previous one, so there is
exactly one, and closing is the same write with `None`. A tab carries its own session, so two tabs may
have different products open.

**A double click in the viewport opens a product.** The first click still selects it; the double click
sets the active product and reveals its children in the tree. Opening a part, a group or the root does
nothing: only a container whose class has `level = Product` can be opened.

**The guard lives in the command, not in the button.** `panel.create`, `object.primitive.create` and
every hardware-creation command refuse to run when the active product is missing, when it is closed
by definition (there is none), or when the target parent is not inside it. The refusal is the command
`enabled` result and a message that names what is missing — «сначала создайте изделие» or «изделие
закрыто» — and nothing is written to the document. A command dispatched by the palette, a shortcut or
a future assistant meets the same check, because it is the same command.

**The guard is about furniture, not about modelling.** A primitive, a drawn figure, a mesh object or
a hardware model can still be created without a product: those are objects of the scene, not parts of
a piece. The guard applies to classes whose `level` is `Part` or `Fitting`.

**Creating a product does not open it.** A product is born at the root or inside a plain group with
its envelope and its class, and the user opens it when they start filling it. This keeps creation one
step and opening one gesture, as the owner described.

**A closed product refuses creation only.** Moving, renaming, deleting and re-parenting a closed
product remain allowed in version 1: the lock is about adding work to a piece, not about owning the
object. This is the owner's open question 5 and the recommendation is to keep it narrow; broadening
it later is additive.

## Consequences

- Every path that creates a part must now name a product: the ribbon's create cells, the palette, the
  empty-space tool and the tests. The fixtures gain a product and a session that opens it, and a test
  that created a panel under the root either moves under a product or asserts the refusal.
- The scene tree shows the active product, and the viewport can dim what belongs to a closed product
  if that proves useful; neither is document data.
- The empty-space tool (the next step) only has to search the active product's envelope, because no
  other part can be created there.
- A scene made before this rule keeps its loose panels: they load, they render, they are simply not
  creatable any more. That is a read rule, not a migration.
- If the assistant later builds a piece, it opens the product as a session view operation before
  dispatching the same create commands — no private path (ADR 0028).

## Related

- [0029](0029-view-state-belongs-to-the-session.md) — why openness is not in the file.
- [0094](0094-a-class-registry-gives-the-machine-its-semantics.md) — `level` is the trait the guard
  reads.
- [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) — the entity being opened.
- [`docs/domain/products.md`](../domain/products.md) — the workspace rules in full.
