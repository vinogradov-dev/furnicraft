# Semantic classes

Status: domain specification. This document defines what an object *is* for the program: the class
registry, its encoding, the traits that carry behaviour, and the rules that keep the registry alive
for the life of the product. It replaces the furniture-type registry of the previous milestone.

Related decisions: [0034](../decisions/0034-nodes-carry-typed-properties.md),
[0094](../decisions/0094-a-class-registry-gives-the-machine-its-semantics.md),
[0095](../decisions/0095-the-order-package-is-a-published-snapshot.md).

## 1. Why a class exists

A node is a body plus a meaning. The body (`Panel`, `Container`, `Object`) says what is drawn and how
it is represented; the class says what the thing is and therefore **what may be done with it**:

- a side panel is cut from sheet stock, is nested, carries edge banding and stretches freely;
- a worktop is a board too, but it is linear stock bought by length, never nested, and must not be
  stretched across its width;
- a hinge is bought by the piece, never nested, never banded, never resized, and admits a cup bore
  and a mounting pattern.

Without the class every one of those sentences becomes a special case written somewhere in the
program, usually about a name or a structural kind. With it, the rules arrive with the object and
every consumer — the inspector, the command `enabled` check, a report, a cutting list, a CNC rule —
reads the same answer.

**The code identifies; the traits decide.** The digits are a stable name, not a compiler: no code path
may compare a code with a literal outside a test. The pattern is
`class.traits().operations().contains(Operation::EdgeBand)`, never `code == 200101`.

## 2. Encoding

Six decimal digits, written `GG.TT.VV` and stored as one integer from `0` to `999_999`:

| Level | Code | Question it answers |
|---|---|---|
| group | `GG.00.00` | the function family: seating, storage, work surface, fitting, appliance, board and trim |
| type | `GG.TT.00` | the thing inside the family: cabinet, shelf, worktop, hinge |
| variant | `GG.TT.VV` | the variant, **only when a rule differs**: hinged versus sliding, overlay versus inset, roller versus ball-bearing runner |

`000000` means unclassified and is the default of every node. It is a sentinel, not the root of the
tree: a query on it matches only nodes without a class.

A zero suffix names the parent: `30.04.00` is the type, `30.00.00` the group, and the dotted form is
presentation only. `100103`, `10.01.03` and `10.01` all parse; display drops a zero variant and a zero
type.

**Three levels are the ceiling.** Anything finer — a 35 mm cup, a damper, a brand, a colour — belongs
to the catalogue article, not to the class. A fourth level is a new schema decision, not an
extension of this one.

**The digits stay.** They sort, compare and prefix-query with no allocation, they read acceptably in
an export table, and six digits match the width the design tradition uses. A mnemonic string in the
stored key was rejected: mnemonics rot (`CAB` stops meaning what it meant) and a stored key cannot be
changed. Each row carries a `slug` (`shelf`, `worktop`, `hinge-spring`) for search, export and program
names; the slug is never stored in a document and never compared.

## 3. The registry row

The registry is a constant table in `furnicraft-model`; it is not a database and not a file the
program loads at run time.

| Field | Meaning |
|---|---|
| `code` | the six-digit identity. Assigned once, never reused, never renumbered |
| `parent` | derived from the digits; a test asserts that the row exists and sits one level up |
| `slug` | mnemonic for people, export tables and generated program names |
| `label_key` | the translation key in `furnicraft-text`; names are data, not code |
| `aliases` | search synonyms (`полка`, `полочка`), never used for matching a document |
| `traits` | the behaviour set below; the only thing algorithms read |
| `since` | the release that introduced the row, for the registry changelog |
| `retired` | withdrawn: resolvable and translated, no longer assignable |

Inheritance runs down the tree and is resolved once at compile time, so `50.01.03` carries everything
it inherits from `50.01` and `50` and a lookup is a bit test. A row may narrow or extend its parent's
traits; a row may not contradict them — the test rejects `nesting = Member` under a parent that is
`Never`.

## 4. Traits, version 1

| Trait | Values | Semantics | First consumer |
|---|---|---|---|
| `supply` | `MadeFromSheet`, `MadeFromLinear`, `BuyPiece` | where the object comes from: a nesting plan, a length cut, or a purchase order | reports, cutting list |
| `measure` | `Area`, `Length`, `Count` | the unit of a report line; derived from `supply` unless a class overrides it with a reason | reports |
| `nesting` | `Member`, `Never` | whether a cutting plan owns the object; derived from `supply` | cutting list |
| `extent` | `Free`, `LengthOnly`, `None` | which dimensions a gizmo may change and which resize commands are offered | gizmos, `enabled` |
| `operations` | `Holes`, `Grooves`, `Contour`, `EdgeBand`, `CupBore`, `MountPattern`, `Cutout` | which machining a class admits | inspector, CNC rules |
| `host` | a set of classes | where the object may sit or be mounted | insertion, mounting |
| `level` | `Product`, `Component`, `Part`, `Fitting` | report grouping and tree presentation | reports |
| `opening` | `Closes`, `None` | whether the object closes an opening, and therefore has gaps and a facade report | reports, drawing |

Rules for the vocabulary:

1. **A trait needs a consumer.** A trait lands with the code that reads it and a test proving the
   wrong class is refused. A trait without a consumer is speculation and is rejected in review.
2. **`measure` and `nesting` are derived** from `supply`, because two independent columns allow an
   impossible row (sheet stock measured by length). An override is allowed with a written reason.
3. **Traits describe physics and economics**, not a workshop's vocabulary: what it is made from, how
   it is measured, what may be done to it, where it may be. That is what makes them survive a decade
   of products.
4. **`level` does not mark a component.** A container carrying a class is a component (0034); `level`
   only tells reports what kind of thing it is.
5. **A trait change is a behaviour change.** Correcting a mistake in an existing row is allowed but
   changes what saved projects do; it is recorded in the registry changelog like a schema change.

## 5. A class earns its code

A class gets a code when **at least one rule differs** from its siblings, or when **a report, a
drawing or a CNC program must name it separately**. When only the wording differs, it is one class
with one label and an alias.

- `30.04` shelf and `30.01` cabinet are both storage; the rules differ in `level`, so both exist.
- `30.04` shelf and a hypothetical `30.05` "shelf for a shoe cabinet" have identical rules and no
  separate report line; that is one class and an alias.
- `60.01.01` worktop and `60.02.01` plinth board are both `MadeFromLinear`; their `extent`,
  `operations` and report line differ, so they are two classes.
- `50.01.02` spring hinge and `50.01.02`-with-a-damper are one class: the damper is an article
  property.
- `50.01.02` spring hinge and its overlay/inset/half-overlay forms are three classes: the mounting
  rules differ.

## 6. Assignment and lifecycle

- `class` is a reserved system property of every node. The inspector assigns it from the registry
  tree (search by label and alias, retired rows hidden, unclassified available); the model rejects a
  code the registry does not know **on assignment**.
- Loading does not validate: an unknown number is kept, the node is marked *not in the registry*, and
  the scene loads. The registry is append-only, so this should never happen for a code that shipped.
- Setting a class on a container marks it as a component; clearing removes the marker unless
  `prototype.id` or `component = true` is also present (0034).
- A code, once in a released build, is **immortal**: never reused, never renumbered, never moved to
  another parent. A withdrawn class is set `retired`, stays resolvable and translated, and stops being
  offered for assignment.
- Each group reserves `TT = 90..99` for local and experimental codes, so a workshop's private class
  never collides with a future official one.

## 7. What the class does not contain

Material, thickness, finish, dimensions, price, supplier, article number, placement, prototype
identity and instance identity. A specific hinge is a catalogue article pointing at the class
`50.01.02`; the class stays the same when the supplier changes.

## 8. Examples

| Object | Body | Class | Rules that make it different |
|---|---|---|---|
| Side panel | `Panel` | `30.0x` side | sheet stock, area, nested, free extent, edge banding |
| Shelf | `Panel` | `30.04` shelf | sheet stock, area, nested; separate report line |
| Back panel | `Panel` | `30.0x` back | sheet stock, contour only, no banding |
| Worktop | `Panel` | `60.01.01` | linear stock, measured by length, never nested, length-only extent, cutouts allowed |
| Spring hinge, overlay | `Container` | `50.01.02.01` | bought by piece, counted, never nested, no extent, cup bore and mounting pattern |
| Runner, undermount | `Container` | `50.02.03` | bought by piece, counted, mounted to a case part |

## 9. Export

The package's `classes` table (see [order export](order-export.md)) carries the code, slug, label,
parent and the traits that a reader needs to place an object in a report. That is deliberate: a report
written in another language decides "worktop goes to linear purchase, never to nesting" from the
table, not from our digits.

## 10. Open questions

- The full class table is a design artefact of its own; it lands with the registry task and the owner
  approves it entry by entry.
- Whether a workshop may add private classes at run time, and if so where that table lives, is not
  decided. The reserved range exists so the answer can be yes later without touching official codes.
- Whether an object may carry more than one class (hardware that is also a fitting family) is still
  open; a second class would need a rule for which one an algorithm reads.
