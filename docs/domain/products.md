# Products

Status: domain specification. A **product** is one piece of furniture — a cabinet, a shelf unit, a
table, a chest — and the unit a maker works on and a report is written for. In the scene it is drawn
as a dashed wireframe box, «Каркас изделия»; in the code and the file it is a container with a class
and an envelope.

Related decisions: [0034](../decisions/0034-nodes-carry-typed-properties.md),
[0094](../decisions/0094-a-class-registry-gives-the-machine-its-semantics.md),
[0097](../decisions/0097-a-product-is-a-container-with-an-authored-envelope.md),
[0098](../decisions/0098-one-product-is-open-and-parts-are-born-inside-it.md).

## 1. The entity

A product is a `Container` whose class carries `level = Product` and which stores an `Envelope`. It is
not a new node kind, not a second container type and not a new id space: it is the meaning the class
gives to an ordinary container, plus one field.

The word for the role is `Product` (`Level::Product` in the registry). The word for its box is
`Envelope`. The interface word is «Каркас изделия» / «Product frame»; `frame` is not used in code,
where it already names a panel frame, a parent frame, a scene frame and a viewport frame.

A product is a component in the sense of 0034 (it carries a class), so `node.ungroup` refuses it and
`node.component.convert_to_group` clears its class and its envelope in one command.

## 2. The envelope

The envelope is three lengths — width, depth, height — in micrometres, written as `x`, `y` and `z` in
the product's own frame.

- **It is authored, never derived.** It is set when the product is created, before any part exists,
  and no edit to a child changes it. This is the invariant the owner stated and it matches
  `AGENTS.md` §4.2: the document stores decisions, everything else is derived.
- **Content bounds are derived.** The bounding box of the children is computed when asked (an idle
  diagnostic, a report, a drawing). It is never stored, so it can never go stale.
- **A child may poke out.** It is a diagnostic — «деталь выходит за габарит изделия» — not a silent
  resize and not a rejected edit. A maker measures the outside first and cuts the inside second.
- **The envelope is validated** like a panel size: positive, finite, inside the documented maxima. A
  bad number is a `Result::Err`, never a clamp.
- **It is drawn, not shaded.** A dashed wireframe parallelepiped in the theme's neutral line, no fill,
  no depth write, and it never picks: it bounds a space, it is not a body.
- **It is a box.** Sloped or otherwise shaped envelopes are out of scope for version 1; the field is
  three lengths, and a shape would be a new field and a new decision.

## 3. Place in the tree

- A product is the top node of its own assembly. Everything inside it — panels, facades, fittings,
  sub-assemblies, nested groups — belongs to it.
- **No product is a descendant of another product.** The model refuses it: a product's parent is the
  root or a plain group.
- **Physical containment is not kinship.** One product may stand in the void of another — in a niche,
  on a shelf, under a worktop — without becoming its child. Its parts belong to the inner product, and
  the outer product's parts list never picks them up. The tree expresses ownership; space expresses
  position.
- `parent_id` and `product_id` are **not stored**. Both are computed by one walk of the tree, in the
  model for the program and in the export for a reader. A stored parent reference would be a second
  source of truth that a re-parent can leave stale.

## 4. The workspace: open and closed

- Loading a scene leaves no product open. A product is opened by a **double click** in the viewport;
  the first click selects, the double click opens.
- **At most one product is open**, and opening another closes the previous one. This is session state
  per tab, never a document field (`docs/domain/document.md`, ADR 0029).
- An open product is the working area: new parts are created inside it and the empty-space tool
  searches its envelope.
- **A closed product accepts no new parts.** Moving, renaming, deleting and re-parenting it stay
  allowed; the lock is about adding work, not about owning the object.
- **Creating a product does not open it.** Creation and opening are two gestures.

## 5. Creating parts

- A panel, a facade or a fitting is created only inside the **open** product. The command's `enabled`
  check refuses when no product is open, when the product is closed (there is none open), or when the
  target parent is not inside the open product.
- The refusal is readable and writes nothing: «сначала создайте изделие» or «изделие закрыто».
- Free modelling is untouched: a primitive, a drawn figure, a mesh object or a hardware model can be
  created without a product. The guard applies to classes whose `level` is `Part` or `Fitting`.

## 6. Filling a void

The owner's scenario: open a product, choose “add a post” or “add a shelf”, point at a place.

- The cursor ray meets the envelope; the point inside it defines the **free space** at that point,
  recomputed against the parts already there: the box reaches the envelope walls and stops at the
  nearest part boundary on each side.
- A **post** takes the void's height and depth; a **shelf** takes its width and depth. Which axis pair
  a class fills is the class trait `fill` (`HeightAndDepth`, `WidthAndDepth`, `None`).
- A **ghost** of the part follows the cursor while it moves, drawn through the session preview overlay
  (ADR 0093): nothing is written until the click.
- The offset step is **1 mm**; there are no fractional sizes.
- A click creates the part with exactly the ghost's size, through one command and one undo entry.
- Version 1 handles axis-aligned voids only, for posts and shelves. A drawer, a back panel and a
  facade are later additions; they need their own fill rules and their own place in the guard.

## 7. Moving a product

The existing move tool stays the tool for geometry. Furniture gets its own:

- A **floor-standing** product (`mount = Floor`) moves only in the floor plane.
- A **wall-hung** product (`mount = Wall`) moves freely, including the vertical.
- Products do not interpenetrate: movement stops on contact instead of passing through.
  The collision is between the parts of the products, not between their envelopes — otherwise a wall
  cabinet could never sit under a worktop, and the niche case of §3 could not exist.
- **Snapping** to the floor, to the faces of neighbouring objects and to the 1 mm grid, so a maker can
  place a piece without fighting the mouse.
- A drag is one undo operation, committed once on release.

## 8. What output sees

The export gives every node a `product_id` (the nearest `level = Product` ancestor) and the product a
row with its envelope and its derived content bounds; a part carries its exact outline and banding.
See [order export](order-export.md). Nothing about the product is special-cased in the exporter: it
reads the class traits, the envelope and the tree, like any other consumer.

## 9. Invariants

1. No stored parent or product reference in the document.
2. No furniture part outside the open product.
3. No product inside a product in the tree.
4. The envelope is authored; it is never derived from children.
5. Envelope presence and `level = Product` agree in both directions.
6. Products may overlap in space only where their parts do not; nothing about an intersection is ever
   stored.
7. Every furniture-tool action writes through a command, and only a command.

## 10. Answered questions

- **A code group for products?** No. `level = Product` is a class trait; the code names the product's
  function (a wall cabinet is `30.01.02`). A code range would put the assembly role back into the
  digits, which 0094 removed, and a reader that needs the answer reads the trait from the class
  table.
- **Sloped envelopes?** Not in version 1. A straight box is the whole field; a slope is a new field.
- **Walls?** The document has no walls. Version 1 snaps to the floor, to neighbouring objects and to
  the 1 mm grid; wall objects are a separate decision.
- **The interface word?** «Каркас изделия» / «Product frame».
- **What a closed product refuses?** Creation only, in version 1.
- **Which parts can fill a void?** A post and a shelf first; a drawer, a back panel and a facade later.

## 11. Open questions

- **Resizing the envelope after creation.** The requirements define it at creation and never say how
  it changes. The command exists (`product.envelope.set`); the gesture does not. Options: numeric
  fields in the inspector, draggable faces of the frame, or both. Shrinking below the parts should be
  a diagnostic, not a refusal.
- **A part of another product inside this one's volume** — the niche case — is legal spatially, but
  the move tool's snapping and the cutting list must agree on it. The rule proposed here (collide with
  parts, not envelopes) needs a confirming test with two products in a niche.
- **What “open” means visually.** Whether the active product is emphasised, and what happens to the
  parts of a closed one, is a UI decision, not a document one.
- **A product's class at creation.** A product needs a class before it has parts; whether the create
  flow asks for the class or offers “unclassified product”, and whether the class can be changed
  later, is undecided.
