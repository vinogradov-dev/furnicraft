# 0097. A product is a container with an authored envelope

- Status: Accepted; amends the class trait list of 0094 and the product rows of 0034/0035;
  its "the frame never picks" clause is amended by 0100
- Date: 2026-11-13

## Context

The editor can already group anything into anything, and a group has no size, no meaning and no
boundary. A furniture maker, however, works on **one piece** at a time: this cabinet is 600 deep and
has a top, its parts belong to it, and nothing else does. Today the program cannot answer "which
cabinet is this shelf in", cannot draw the space a new piece will occupy, and cannot refuse a part
that floats in the void.

The owner's requirements ("a product, drawn as a wireframe frame in the scene, with a size, created
before its parts") were written while the old `FurnType` registry was current, so they name a code
group 10 for products. The class registry (0094) removed exactly that: the top level of the code is
the function of the object, and the role an object plays in an assembly is a trait. A cabinet is a
storage object (`30.01`) that plays the role of a product; a shelf is a storage object that plays the
role of a part.

## Decision

**`Product` is not a node kind.** A product is a `Container` whose class carries `level = Product`
and which stores an `Envelope` — three lengths, width, depth and height, in micrometres. No new
`Node` variant, no second container type, no new id space. The entity is expressed by two things the
model already has (a container and its class) plus one field.

**The envelope is authored data, never derived.** It is set when the product is created, before any
part exists, and no edit to a child changes it. The derived *content bounds* of the children are
computed on demand, never stored; a child that pokes out of the envelope is a diagnostic, not a
reason to resize it. This keeps the invariant that the document stores what a human decided and
derives everything else (AGENTS.md §4.2).

**The role is a trait, not a code range.** Group 10 is not revived. `level = Product` on the class is
what makes a container a product, and the class code names the product's function — a wall cabinet is
`30.01.02`, a work table is `40.01.01`. A product is therefore discovered by a trait query, and an
export reader that needs the same answer reads `level` from the `classes` table. If a future external
system wants "is this a product?" without our registry, the trait is the answer and the digits are
not.

**Mounting is a class trait.** `mount: Floor | Wall` says whether the piece stands on the floor or
hangs, and the furniture move tool reads it (0098, §5 of the requirements). It is not a node field:
the registry variants already distinguish a base cabinet from a wall cabinet, and a second source of
truth would let one object claim both and make the move rule ambiguous. A piece that genuinely comes
in both forms gets two variant classes.

**Two traits join the vocabulary.** `mount` (above) and `fill: None | HeightAndDepth | WidthAndDepth`,
which tells the empty-space tool how a part fills a void: a post takes the void's height and depth, a
shelf its width and depth. Both arrive with the tool that reads them, per the rule that a trait needs
a consumer.

**Invariants, checked by the model.**

- An envelope exists only on a container whose class has `level = Product`; the root and ordinary
  groups carry none. A product class without an envelope is not a product yet, and a container with
  an envelope and another class is a readable error.
- A product is never a descendant of a product in the tree. It sits at the top of its own assembly:
  the root or a plain group may be above it, nothing else. Everything inside it — panels, facades,
  fittings, sub-assemblies — belongs to it.
- A product is a component in the sense of 0034 (it carries a class), so `node.ungroup` refuses it;
  converting it to a group clears the class and the envelope in the same command.
- The envelope is validated like a panel size: positive, finite, within the documented maxima. A bad
  number is a `Result::Err`, never a clamp.
- Physical containment is not kinship. One product may stand in the void of another — a niche, a
  shelf, the space under a worktop — without becoming its child; the parts of the inner product
  belong to the inner product, and the outer product's parts list never collects them.

**The frame is drawn, not shaded.** The scene shows the envelope as a dashed wireframe parallelepiped
with no fill, in the theme's neutral line colour, and it never picks: it is the boundary of a space,
not a body. The interface calls it «Каркас изделия» / «Product frame»; the code says `Product` and
`Envelope`, because `frame` already names four different things in this tree (a panel frame, a parent
frame, a scene frame, a viewport frame).

**Version 1 is a straight box.** Sloped or reshaped envelopes are not supported; the envelope is three
lengths, not a shape. A future slope is a new field and a new decision.

## Consequences

- The schema gains one optional field on `Container` and the class table gains two traits; by 0096
  this costs no migration, only a bump and a regenerated golden file.
- The document can answer "which product is this part in" with one walk, which is what a per-product
  report, the export's `product_id` column and the creation guard all need. Nothing is stored, so no
  reference can go stale (the summary's requirement that `parent_id`/`top_id` are computed holds).
- An empty product is a legitimate document: it is how a maker starts a piece. The editor must not
  treat a product with no parts as an error.
- Duplicating a product copies its envelope; placing it next to the original is one command and the
  move tool keeps them from intersecting.
- The template/recipe milestone (user scripts over properties) gets its anchor: a script creates a
  product with an envelope and fills it, instead of spraying parts into the root.
- The word "frame" stays overloaded in the interface only; a reader of the code finds `Envelope`.

## Related

- [0034](0034-nodes-carry-typed-properties.md) — one container kind and the component marker; a
  product is a component with an envelope.
- [0094](0094-a-class-registry-gives-the-machine-its-semantics.md) — the class and its traits; this
  record amends its trait list with `mount` and `fill`.
- [0096](0096-before-the-first-release-the-format-has-no-past.md) — why the new field needs no
  migration.
- [0098](0098-one-product-is-open-and-parts-are-born-inside-it.md) — the workspace rule that uses it.
- [`docs/domain/products.md`](../domain/products.md) — the specification and its open questions.
