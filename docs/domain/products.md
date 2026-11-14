# Products

Status: domain specification. A **product** is one piece of furniture — a cabinet, a shelf unit, a
table, a chest — and the unit a maker works on and a report is written for. In the scene it is drawn
as a dashed wireframe box, «Каркас изделия»; in the code and the file it is a container with a class
and an envelope. The empty space it reserves is a **niche**, and the niche is what its parts are
measured against.

Related decisions: [0034](../decisions/0034-nodes-carry-typed-properties.md),
[0094](../decisions/0094-a-class-registry-gives-the-machine-its-semantics.md),
[0097](../decisions/0097-a-product-is-a-container-with-an-authored-envelope.md),
[0098](../decisions/0098-one-product-is-open-and-parts-are-born-inside-it.md),
[0099](../decisions/0099-a-niche-is-the-space-a-frame-reserves.md),
[0100](../decisions/0100-a-frame-is-created-opened-and-drawn.md),
[0106](../decisions/0106-a-niche-is-a-convex-cell-of-wall-planes.md).

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
  and no edit to a child changes it. This is the owner's invariant and it matches `AGENTS.md` §4.2:
  the document stores decisions, everything else is derived.
- **Content bounds are derived.** The bounding box of the children is computed when asked — an idle
  diagnostic, a report, a drawing. It is never stored, so it can never go stale.
- **A child may poke out.** It is a diagnostic — «деталь выходит за габарит изделия» — not a silent
  resize and not a rejected edit.
- **It is validated** like a panel size: positive, finite, inside the documented maxima. A bad number
  is a `Result::Err`, never a clamp.
- **Its faces are virtual panels.** The six planes of the envelope stop a probe, accept a snap and
  take a part flush against them exactly as a real panel would (0099). This is the reason a reserved
  space is useful: the maker cuts to it.
- **It is a box.** Sloped or otherwise shaped envelopes are out of scope; the field is three lengths
  and a shape would be a new field and a new decision.
- **It anchors at its base min corner.** The local zero is the corner of the base rectangle, as a
  panel's zero is its base corner (0083), so the reserved space is `[0..W] × [0..D] × [0..H]` in the
  frame and nothing needs an offset. Dragging a max face changes the size; dragging a min face changes
  the size and translates the placement so the opposite face stays put in the world (0082, 0088).
- **It is edited like a box**, through the shared box tool (0101): a corner grip changes two sizes, a
  face grip one, and the card three numbers. Each gesture is one undo entry. The grips show whenever
  the frame is selected, even when its dashed lines are hidden by the closed-with-parts state (0100).

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
  model for the program and in the export for a reader.

## 4. The niche

The niche is the feature the program is built around, and it is the workshop's own word: a product is
the place reserved for a piece, and its inside — once the case exists — is a niche. When a shelf
crosses the inside at full width, the piece has two niches: above and below.

**A niche is derived, never stored.** It is the connected empty space around a point inside a
product, bounded by the envelope's virtual faces and by the parts or other products that stop it. No
id, no node, no command, no field; like content bounds, it is computed when asked, so it cannot go
stale.

**It is measured by probes.** From the point, rays are cast along the product's own axes and corners,
and each stops at the first furniture object, part or envelope face. The free space is the
intersection of the half-spaces behind the faces those rays met: a **convex cell** in the product's own
frame. The measurement starts from the envelope's six planes, clips the cell by each met face's plane,
probes again from the cell's own corners, and repeats until a round adds nothing (0106). Each face must
still be a surface over its whole extent, which is what refuses a board that stops halfway.

**The shape is the cell, not the box around it.** A product turned off the world axes is measured in
its own frame, so it is measured like any other; a leaned back wall or a ceiling that comes down
towards the back bounds the cell exactly, wherever the cursor happens to stand. The cell answers its
**supporting planes** (a unit normal pointing into the void and an offset), its **corners**, and the
**polygon a board of a given thickness takes**: a shelf asks for the cell cut at its own slab, a post
for the same cut seen in its own board plane. A rectangular case answers a plain rectangle, a case
whose posts stand at an angle answers the trapezoid, and a board in a case with a sloped ceiling stops
where the ceiling comes down instead of cutting into it.

**A void that is not convex is refused, not approximated.** No other surface's true corner may lie
strictly inside the cell: a void holding a post or a shelf would let a part cut into it. A notch, two
boards meeting halfway, or a wall that stops before the face it would have to bound are refused the
same way, with a sentence the tool shows rather than a part that does not fit. A curved boundary is
measured by the box around its body, which encloses it — the part fits with a gap and never cuts in.
Splitting a non-convex void into several cells is the next stage and is not planned yet. Arbitrary
regions are not planned. Nothing in the document changes: the niche stays derived.

**A probe stops at the first surface of a furniture entity and never passes through it** (0102): a
panel face, a hardware body or another product's shell. A piece standing inside another's reserved
space therefore reduces the niche of the outer piece exactly as a panel would, which is what makes the
niche "bounded by the envelope and by panels and other furniture products" true in the program and not
only in the sentence.

**A niche answers with its cell and its box.** The **size** (width, height, depth) is the box around
the cell, which is what the callouts print; the **world position** is that box's corner, so a tool
never re-derives a position from a cursor point and a size; the **shape** says whether the box is the
free space, whether one section describes it from floor to ceiling, or whether the section changes
along the height; and the cell itself — its planes, its corners and the cut a board takes — is what a
part is measured against. The answer is a model query (0103), so the fill tool, the distance indicator
and the move tool all consume the same one.

**It is shown, not stored.** Hovering a void with a fill tool draws the niche: a wireframe box on its
edges, no fill, drawn slightly in front of the piece so the panels around it do not hide it, with three
dimension callouts — height, width, depth in whole millimetres. The box uses the measurement role of
the theme and never picks; the callouts disappear when the gesture ends. The forward offset is a
visibility device, not a statement that the niche extends beyond the envelope.

**A niche is a question, not a resource.** Nothing reserves it; two parts may claim the same empty
space, and the overlap is a diagnostic, not a refused edit. The shop does not work by reservation,
and a reservation system would be a second document model with its own staleness problem.

**Which niches are offered.** Only the open product's (0098): a closed product is not a working area,
so its voids are not offered as targets.

## 5. The workspace: create, open, close

**A frame is created open, in two ways.** The ribbon's *Product frame* cell opens a card: width,
depth and height in 1 mm steps, and a type field listing product classes. Its second method,
*Frame by drawing*, arms the shared box tool (0101): base rectangle on the work plane, then the height
pulled from a wireframe preview. Both paths dispatch `product.create`, and nothing else may create a
product.

**One class comes from the card, the placeholder from the gesture.** A typed frame takes the class the
maker picked; a drawn one takes «изделие общего назначения» / “general product”, the editor's
placeholder class, because a pointer gesture names no type. The envelope invariant holds either way,
and the type is a later edit rather than a prerequisite.

**Placement is the gesture's or the card's.** The card puts the frame at the origin in an empty scene,
and otherwise beside the nearest product, on the floor, clear of every other envelope. A drawn frame
stands where its base rectangle was drawn. Either way it becomes selected and open, and the maker
moves it with the furniture move tool (§8).

**Opening is a double click in the viewport**, or the tree. At most one product is open at a time, and
opening another closes the previous one. Openness is session state per tab, never a document field
(ADR 0029).

**A closed product accepts no new parts.** Moving, renaming, deleting and re-parenting it stay
allowed; the lock is about adding work, not about owning the object.

## 6. Creating parts

- A panel, a facade or a fitting is created only inside the **open** product. The command's `enabled`
  check refuses when no product is open or when the target parent is not inside it, and the message
  says which: «сначала создайте изделие» or «изделие закрыто».
- The refusal writes nothing: no revision, no history entry.
- Free modelling is untouched: a primitive, a drawn figure, a mesh object or a hardware model can be
  created without a product. The guard applies to classes whose `level` is `Part` or `Fitting`.

## 7. Filling a niche

The maker's flow: open a product, choose “add a post” or “add a shelf”, point at the empty space.

- The cursor ray meets the envelope's interior; the point defines the niche (§4).
- A **post** takes the niche's height and the depth the void leaves it; a **shelf** takes its width and
  depth. Which axis pair a class fills is the class trait `fill` (`HeightAndDepth`, `WidthAndDepth`,
  `None`). Each takes the cell's own cut at the board's thickness, so a post stands against a splayed
  wall with the depth the slab really has (0106).
- The part has **two modes**, chosen from the tool's own method list and remembered by the workspace:
  - **Largest fit** is the default: the part is the largest rectangle, with its sides on the panel's
    own axes, that lies inside the cut section. It always fits, and it always keeps the standard
    rectangle panel — resize, card and nominal fields included.
  - **Whole shape** makes the part the section itself. Before the panel is chosen, the program asks
    whether a standard rectangle draws exactly that shape: the plain rectangle, a rectangle with
    corner chamfers (asymmetric ones included) or one with corner radii. The candidates are
    generated and compared with the measured ring vertex by vertex within a micrometre, so a shape
    is never called standard on an approximation. Only a section no rectangle draws — the trapezoid
    of two angled posts, a cut that consumes a whole edge — becomes a contour panel, which trades
    `panel.resize` and the rectangle card for the exact shape (0042).
- A **ghost** of the part follows the cursor through the session preview overlay (ADR 0093); nothing
  is written until the click.
- The offset step is **1 mm**; there are no fractional sizes.
- A click creates the part with exactly the ghost's size, outline and mode, through one command and
  one undo entry: the outline — a rectangle with its corner cuts, or the ring — travels in the same
  `panel.create`, so a shaped part is never a create followed by a second edit.
- The two fills handle a product in any orientation, because the cell is measured in the product's own
  frame; a curved boundary is measured by the box around its body, so the part fits with a gap. A
  drawer, a back panel and a facade need their own fill rules.

**Exact distances.** While a fill, a box creation or a move runs, the viewport shows one distance field
per direction of the work plane — from the new face to the first surface — and exactly one of them is
active (0103). The fields are the shared number field with the unit outside, so the mechanism is the
one the cards already use.

- Typing into a field takes the mouse out of the gesture for that direction; Enter commits the
  gesture, Escape returns the mouse, Tab moves to the opposite direction.
- A field also accepts a semantic value, «По центру» / “Centred”, from its menu: the part is placed in
  the middle of the void and the result is a computed coordinate, not a number.
- A committed distance shows a lock, and the next one is measured from the locked face rather than
  from the original reference, so two typed distances cannot drift apart.
- A direction with no neighbour disables its field and says why. A value that is negative, larger than
  the void or would overlap a part is refused with a message, and nothing is written.
- Every value is a whole millimetre, and one gesture is one undo entry.

## 8. Moving a product

The existing move tool stays the tool for geometry. Furniture gets its own:

- A **floor-standing** product (`mount = Floor`) moves only in the floor plane.
- A **wall-hung** product (`mount = Wall`) moves freely, including the vertical.
- Products do not interpenetrate, and a furniture entity is a surface (0102): a **body** stops at
  contact with another body, and a **shell** stops at contact with another shell, so no product is
  half inside another's envelope. A shell may still *contain* another shell — a wall cabinet under a
  worktop, a piece in a niche — because containment is not crossing.
- **Snapping** to the floor, to the frame's virtual faces, to the faces of neighbouring parts and to
  the 1 mm grid.
- A drag is one undo operation, committed once on release.

## 9. Drawing and picking

| State | Drawn |
|---|---|
| Open | envelope lines in the active line colour |
| Closed, no parts | envelope lines in the inactive line colour |
| Closed, with parts | no envelope lines |

Selecting a frame shows its grips — eight corners and six faces — which edit the envelope through the
shared box tool (0101): a corner changes two sizes, a face one, the card three numbers, and each drag is
one undo entry.

The frame is picked **by its envelope**, not only by its lines: a single click inside the piece, on
empty space or on a line, selects the frame; a click on a part selects the part. Solids win a tie, so
a click that hits a panel picks the panel. When several products contain the point, the innermost
wins — that is how a product standing in the void of another stays reachable. A double click opens the
product under the cursor. None of this is document data.

## 10. What output sees

The export gives every node a `product_id` (the nearest product ancestor) and the product a row with
its envelope and its derived content bounds; a part carries its exact outline and banding. See
[order export](order-export.md). The exporter special-cases nothing: it reads the class traits, the
envelope and the tree, like any other consumer. A niche is not exported; a report that needs its
volume asks the program for it when such a report exists.

## 11. Invariants

1. No stored parent or product reference in the document.
2. No furniture part outside the open product.
3. No product inside a product in the tree.
4. The envelope is authored; it is never derived from children.
5. Envelope presence and `level = Product` agree in both directions.
6. Products may overlap in space only where their parts do not; nothing about an intersection is ever
   stored.
7. A niche is never stored; it is computed from the envelope and the parts.
8. Every furniture-tool action writes through a command, and only a command.

## 12. Answered questions

- **A code group for products?** No. `level = Product` is a class trait; the code names the product's
  function (a wall cabinet is `30.01.02`). A code range would put the assembly role back into the
  digits, which 0094 removed.
- **The class at creation?** A placeholder class, «изделие общего назначения», assigned by
  `product.create` and changed later from the card; a product always has a class.
- **Sloped envelopes?** Not in version 1. The niche's shape is a separate, staged promise (§4).
- **Walls?** The document has no walls. Version 1 snaps to the floor, to the frame's own faces, to
  neighbouring parts and to the 1 mm grid; wall objects are a separate decision.
- **The interface word?** «Каркас изделия» / «Product frame».
- **What a closed product refuses?** Creation only, in version 1.
- **Which parts can fill a niche?** A post and a shelf first; a drawer, a back panel and a facade
  later.
- **What the placeholder class is called?** «Изделие общего назначения» / “general product”, a
  `level = Product` row that is never offered for a part.
- **How a new frame is placed?** Beside the nearest product, on the floor, clear of every envelope,
  and selected; the origin in an empty scene.
- **Where a probe stops?** At the first surface of a furniture entity; it never passes through one
  (0102).
- **How the envelope changes after creation?** By a corner grip (two sizes), a face grip (one size) and
  the card's three fields, through the shared box tool (0101).

## 13. Open questions

- **Shrinking the envelope below the parts.** It is a diagnostic, not a refusal, but the frame's grips
  make it easy to do by accident; whether the tool warns at the drag or only after the drop is open.
- **The forward offset of the niche box.** Confirmed as a visibility device: the box is drawn in front
  of the piece so the surrounding panels do not hide it; the niche itself stays inside the envelope.
- **Moving a product into another's reserved space.** The shell rule (0102) allows it and the mover
  must stop at walls, not at envelopes; the snapping of a shell face to another shell face needs one
  confirming test.
- **Other semantic values in a distance field.** «По центру» is the first; equal gaps between several
  parts and "flush with the envelope face" are plausible next ones and are not decided.
- **What “open” looks like beyond the colour.** Whether the active product is emphasised, and what
  happens to the parts of a closed one, is a UI decision.
- **A part of another product inside this one's niche** — the niche case — is legal spatially, but the
  move tool's snapping and the cutting list must agree on it; it needs a confirming test with two
  products in a niche.
