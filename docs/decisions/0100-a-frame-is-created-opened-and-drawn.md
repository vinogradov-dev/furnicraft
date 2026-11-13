# 0100. A frame is created, opened and drawn: the lifecycle

- Status: Accepted; amends 0097 (the frame picks) and 0098 (creation opens; the card and the default
  class); its creation flow is extended by 0101, where the frame
  also draws its base and pulls its height with the shared box tool
- Date: 2026-11-13

## Context

0097 and 0098 settled the entity and the workspace rule but left three things to the implementer: how
a frame comes into being, what open and closed look like, and whether the dashed box can be clicked.

The owner has now described the flow a maker actually wants: press *add frame*, a card asks for the
dimensions, the frame appears — at the origin in an empty scene, next to the nearest piece otherwise —
and it is **open**, ready for parts. The frame is classified on creation as a general-purpose product
and the card carries a type field, so the maker can say what it is now or later.

The same description settles the drawing: a frame is an assembly, so open and closed mean what they
mean for an assembly, and the picture has to say which state it is in — and the frame has to be
clickable, or it can never be selected, opened or moved.

## Decision

**A frame is created open.** Creating a product sets it as the session's active product (0098), so the
next part goes inside it without another gesture. This replaces the earlier "creation does not open".

**Creation is a command and a card.** `product.create` takes a name, an envelope, a class and a
placement; the card in front of it edits width, depth and height in 1 mm steps and offers a searchable
list of product classes. Without a choice the frame is **«изделие общего назначения» / “general
product”**, a placeholder class reserved for the editor — a product always has a class, so the
envelope invariant of 0097 keeps holding instead of gaining an exception, and the type field is a
later edit, not a prerequisite.

**Placement is deterministic.** In an empty scene the frame stands at the origin, on the floor. With
other products in the scene it is placed next to the nearest one, on the floor, clear of every other
envelope, and it becomes selected; the maker moves it from there. Activating the move tool instead is a
plausible alternative and stays an open question, not a hidden second behaviour.

**Open and closed are a class of assembly state, drawn honestly.**

| State | Drawn |
|---|---|
| Open | envelope lines in the active line colour |
| Closed, no parts | envelope lines in the inactive line colour |
| Closed, with parts | no envelope lines — the piece speaks for itself |

The active product is the one open product (0098); its lines are the ones in the active colour.

**The frame is picked by its envelope, not only by its lines.** A single click inside the piece, on
empty space or on a line, selects the frame; a click on a part selects the part. Solids win a tie: a
click that hits a panel picks the panel, and the frame is picked only where nothing solid is under the
cursor. When several products contain the point, the innermost wins — that is how a product standing
in the void of another stays reachable. A double click opens the product under the cursor.

**The state is session data.** Nothing about open, closed, active colour or selection enters the
document (0098, ADR 0029).

## Consequences

- The ribbon gains one cell — *add frame* — and the frame tool is the first thing a new project uses,
  which is also the first thing the creation guard (0098) requires. A new scene now has a natural
  first step instead of an empty root.
- The placeholder class is registry data: the table of 0094 gains a row, and the editor never offers it
  for a part.
- A closed product with parts can still be selected and reopened, because its interior is pickable
  even when its lines are hidden. A closed *empty* product is small and unmistakable.
- Because creation opens the frame, the ordering of the two commands matters: `product.create` then a
  session view operation, never a command that writes "open" into the file.
- The frame's role in the scene is now threefold — boundary for niches (0099), snap target, and
  selection handle — which is why it must be exact: the envelope is the piece's true size.

## Related

- [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) — the entity this creates.
- [0098](0098-one-product-is-open-and-parts-are-born-inside-it.md) — the workspace rule it amends.
- [0099](0099-a-niche-is-the-space-a-frame-reserves.md) — the niche the open frame offers.
- [`docs/domain/products.md`](../domain/products.md) — the specification and the open questions.
