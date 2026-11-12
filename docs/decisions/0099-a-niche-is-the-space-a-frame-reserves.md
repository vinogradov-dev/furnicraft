# 0099. A niche is the space a frame reserves

- Status: Accepted; refines 0097 and the empty-space step of its plan
- Date: 2026-11-13

## Context

The first version of the product requirements treated "free space at the cursor" as a detail of one
tool: point, get a box, place a post. The owner has since named it as the feature the program is
built around, and the name is the workshop's own: a **niche** is the empty space a piece of furniture
has or reserves — the place a cabinet will occupy, the inside of a case, the gap above a shelf.

That changes what it is. A niche is not a tool's scratch box; it is the thing a part is measured
against. A post is cut to the reserved place, a shelf spans the inside of the case, and once a shelf
crosses the case at full width the case has two niches instead of one. In a real shop this needs no
bookkeeping: the niche is simply where there is nothing yet. The program must see the same thing.

The requirement also breaks the box: two posts standing at an angle form a trapezoid, and a shelf in
that niche must take the trapezoid. Six distances cannot describe that.

## Decision

**A niche is a derived region, never stored.** It is the connected empty space around a point inside
a product, bounded by the product's virtual envelope faces and by the parts or other products that
stop it. It has no id, no node, no command and no field in the document; like content bounds
(0097), it is computed when asked, so it can never be stale. Splitting a case with a shelf is not an
edit to the niche — the same query at a different point simply returns a different region.

**It is measured by probes.** From the point, six rays are cast — right, left, up, down, forward,
back — and each stops at the first furniture object or at the envelope. In version 1 the six
distances give an axis-aligned box, which is what the post and the shelf tools consume. Probes are
the interface because a ray is what both a box and an oblique boundary answer with.

**The shape is a staged promise.** Because the probes report *faces*, not only lengths, the same
query can return a cross-section: the first non-rectangular case is a product whose posts stand at an
angle, where the free section is a trapezoid extruded along the depth. Version 2 builds the outline
from the probed faces and lets a shelf take it; version 3 would be arbitrary regions and is not
planned. Nothing in the document changes between the stages: the niche stays derived.

**The envelope has virtual faces and they are real for editing.** Although the frame is drawn as
dashed lines, its faces snap, accept parts and stop a probe exactly like a panel does. A panel created
against the frame's side is flush with the reserved space, which is the point of reserving it.

**Probes stop at parts, not at other envelopes.** Another product's parts are obstacles; its empty
interior is not. That is what makes the niche that holds a wall cabinet inside a run of base units
possible at all, and it is the same rule the furniture move tool uses (§0097).

**A niche is a question, not a resource.** Nothing allocates it, nothing reserves it and two parts can
claim the same empty space — the program reports the overlap as a diagnostic and lets the maker
decide. The alternative, a reservation system, would be a second document model with its own staleness
problem, and the shop does not work that way.

## Consequences

- The empty-space tool becomes "fill the niche": the ghost it draws is the probed region, and the
  click creates a part measured by it.
- A cutting list can report a niche's volume and a drawing can dimension it later; both read the same
  query, so they cannot disagree with the tool that placed the part.
- The product frame stops being decoration: its faces bound every niche of the piece, so the envelope
  is load-bearing for editing, not only for reports.
- Version 1 can do posts and shelves in boxes. The trapezoid case is a real requirement and gets its
  own step; until it lands, a shelf in a slanted run would be a box that does not fit, and the
  interface must say so rather than guess.
- A niche never appears in the file, so a saved project cannot carry a stale or contradictory one.
  An external consumer that wants niches asks the program for them, or the export learns a niche
  table when a report needs it — not before.

## Related

- [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) — the envelope whose faces bound
  the niche.
- [0098](0098-one-product-is-open-and-parts-are-born-inside-it.md) — only the open product is a
  working area, so only its niches are offered.
- [0093](0093-a-preview-fills-a-session-overlay.md) — the overlay the ghost is drawn through.
- [`docs/domain/products.md`](../domain/products.md) — the specification.
