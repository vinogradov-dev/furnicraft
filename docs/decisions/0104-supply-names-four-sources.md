# 0104. Supply names four sources, and a product is assembled

- Status: Accepted; amends the trait list of 0094
- Date: 2026-11-13

## Context

0094 gave `supply` three values — `MadeFromSheet`, `MadeFromLinear`, `BuyPiece` — because it described
where a *board or a fitting* comes from. 0097 then put products into the registry as a class trait
(`level = Product`) and the class table had to give every product row a supply.

None of the three fits. A cabinet is not sheet stock: its parts are. A chair is not linear stock. A
product is not bought by the piece either. Forcing a product into `MadeFromSheet` also breaks the
derivation in 0094, because `nesting` and `measure` follow `supply`: the whole product would enter a
cutting plan as a nested member and a report would measure a chair in square metres.

## Decision

`supply` gains a fourth value, **`MadeInShop`**: the object is assembled in the workshop from parts
that carry their own classes. `measure` derives to `Count` and `nesting` to `Never` from it, exactly
as from `BuyPiece`.

Two kinds of row carry it. A row with `level = Product` does, because a piece of furniture is
assembled rather than cut or bought. So does a *part with a recipe* — a tamburat panel of two 8 mm
skins and rails, a bent panel, a made drawer box — because the object is one thing the program draws
and counts and several things the shop cuts; its components are what the cutting list expands, and
nesting the whole panel would report stock that is never cut that way.

The vocabulary is therefore `sheet`, `linear`, `piece`, `shop` in the table of
[`docs/domain/classes.md`](../domain/classes.md) §6, and `MadeFromSheet`, `MadeFromLinear`, `BuyPiece`,
`MadeInShop` in the code.

## Consequences

- A report can separate three purchase channels from one assembly channel, and an export reader that
  writes a purchase order never picks up a product or a shop-made panel (0095 ships the value in the
  `classes` table).
- A product's and a recipe part's report line is a count of pieces, never an area, and a cutting list
  never contains them — it contains their components.
- The value is a promise like every other trait: a later change to it is a behaviour change for saved
  projects.
- The other three values keep their meaning exactly; no existing row changes because no row existed.

## Related

- [0094](0094-a-class-registry-gives-the-machine-its-semantics.md) — the trait set and its derivation.
- [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) — a product is a container with a
  class of level `Product`; the trait list it already amends with `mount` and `fill`.
- [`docs/domain/classes.md`](../domain/classes.md) — the registry table that uses the value.
