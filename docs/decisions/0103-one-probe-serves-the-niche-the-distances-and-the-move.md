# 0103. One probe serves the niche, the distances and the move

- Status: Accepted; refines 0099 and 0102
- Date: 2026-11-13

## Context

The niche was specified as its own query, the furniture move needed a collision test, and a maker still
needs to say "5 mm from the post" instead of trusting the mouse. Studied as behaviour — not as a
specification — an established furniture editor answers all three from one idea: the distance from a
cursor to the nearest existing surface, in each direction, with the numbers shown live and editable.

The owner's rule from the same discussion is the same idea twice: the ray stops at the first surface of
a furniture entity (0102), and the niche is what those rays enclose. A selection mechanism that
returns a niche's **size, its coordinates in space and its shape** is therefore not a fill-tool detail
— it is the program's measuring instrument, and several tools must share it.

## Decision

**One probe, in the model.** `furnicraft-model` answers, for a point and a direction, the distance to
the first furniture surface and what it hit — a panel face, a hardware body, another product's shell,
or the virtual face of the product's envelope. It is a pure query over the document: no session, no
renderer, no allocation per call beyond the walk it needs.

**The niche is one consumer.** Three or more probes along the axes give a niche, which answers with
its **size**, its **world position** (the corner the box is anchored at), and its **shape** — a box
today, an extruded cross-section once 0099's second stage lands. The niche is still derived and still
never stored.

**The distance indicator is another.** While a fill, a box creation or a move gesture is running, the
viewport shows one field per direction from the new face to the first surface. The fields are the
shared number field (ADR 0092), one of them active; typing takes the mouse out of the gesture for that
field, Enter commits, Escape returns to the mouse, Tab moves to the opposite direction. A field also
accepts a semantic value — «По центру» / “Centred” — which places the part in the middle of the void
and yields a computed coordinate rather than a number. A fixed distance shows a lock, and the next one
is measured from the fixed face instead of the original reference, so two typed distances cannot drift
apart. A direction with no neighbour disables its field and says why; a value that is negative, too
large or would overlap a part is refused with a message and writes nothing. Every value is a whole
millimetre, and one gesture is one undo entry.

**The move tool is the third.** Its floor constraint, its contact stop and its snapping read the same
probe: a body stops at a body, a shell at a shell (0102), and a snap target is the surface the probe
found, not a separately computed one.

**Tools are thin.** A tool decides what to do with an answer — place a part, pull a height, stop a
drag — and never re-implements the measurement. A tool that needs a new question adds it to the probe
with a test, not to itself.

## Consequences

- One place fixes every measurement: a probe bug is one bug, and the niche, the indicator and the
  mover cannot disagree about where a surface is.
- The niche's answer carries its world position, so a tool never has to re-derive it from a cursor
  point and a size, which is exactly where rounding drift appears.
- The indicator is session UI over model answers, so it is testable without a GPU: the numbers come
  from a pure function, and only their placement on screen needs a window.
- A typed distance is a placement, not stored data; the lock is session state of the gesture; the
  centred value is a mode that yields a coordinate. Nothing about any of them enters the document.
- Because the probe is a ray against the document, its cost is a walk of the open product's nodes. It
  is queried when the pointer moves to a new void, not on every frame.
- The awkward cases are decisions, not accidents: no neighbour, an impossible value, a typed value
  followed by a moving mouse, Tab on a direction without a neighbour, a rotated work plane whose axes
  are the plane's own.

## Related

- [0099](0099-a-niche-is-the-space-a-frame-reserves.md) — the niche as a consumer of the probe.
- [0102](0102-a-furniture-entity-is-a-surface.md) — what a probe stops at.
- [0092](0092-a-field-is-a-number-with-its-unit-outside.md) — the field the indicator reuses.
- [0093](0093-a-preview-fills-a-session-overlay.md) — the overlay the gesture draws through.
- [`docs/domain/products.md`](../domain/products.md) — the niche and the exact distances in prose.
