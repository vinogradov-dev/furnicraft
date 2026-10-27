# Furniture types

Status: domain specification. This document defines FurniCraft's numeric furniture-type
classification: what a `FurnType` is, how it is encoded, which codes exist today and how commands
and reports use them. It does not describe how any product is built; product prototypes are user
scripts and live outside the document model.

Related decisions: [0034](../decisions/0034-nodes-carry-typed-properties.md),
[0035](../decisions/0035-furniture-types-form-a-numbered-tree.md).

## 1. Purpose

Reports, cutting lists and CNC rules need to answer questions such as "every shelf", "every facade",
"every hinge", "all hardware" without knowing the name a person gave a node. A stable, machine-read
code answers them. The code is also the primary marker that turns an ordinary container into a
**component** (an assembly in the older vocabulary).

A `FurnType` is therefore:

- **numeric and stable** — a code is assigned once and never reused for another meaning;
- **hierarchical** — every code has exactly one parent, so a query can match a subtree;
- **opaque to geometry** — it never encodes size, material, finish or placement;
- **independent of prototypes** — it classifies the object, while `prototype.id` points at a future
  script and database record.

## 2. Encoding

A furniture type is six decimal digits, written `GG.TT.VV`:

| Part | Digits | Meaning |
|---|---|---|
| `GG` | 1–2 | group: the broad family of the object |
| `TT` | 3–4 | type inside the group |
| `VV` | 5–6 | variant inside the type |

The value is stored as an integer from `0` to `999_999`. `000000` means **unclassified** and is the
default for every node.

A zero suffix means "the type itself, without a variant":

- `10.01.00` is the cabinet type;
- `10.01.03` is a tall cabinet, a variant of the cabinet type;
- `10.00.00` is the product group.

The dotted form is for people. The stored integer has no separators, so it sorts and compares
cheaply. Parsing accepts both `100103` and `10.01.03`; formatting drops a zero variant and a zero
type, so `100103` displays as `10.01.03`, `100100` as `10.01` and `100000` as `10`.

## 3. Tree rules

The hierarchy is strict:

- every variant `GG.TT.VV` with `VV != 00` has the parent `GG.TT.00`;
- every type `GG.TT.00` with `TT != 00` has the parent `GG.00.00`;
- every group `GG.00.00` has the parent `000000`.

There are no sibling ranges and no second colon-separated part: a code always describes one node
in one tree. A query matches the node itself and all of its descendants, so `30` finds every facade
and `40.02` finds every runner.

## 4. Registry

The table below is the registry for this milestone. Codes are registry data, not hardcoded
semantics: commands and reports match by number, and labels can be translated or renamed without
changing a saved file.

### 10 — Product

A whole piece or a sub-assembly with its own identity. Products are containers; their parts sit
below them.

| Code | Type | Variants |
|---|---|---|
| `10.01` | Cabinet | `.01` base, `.02` wall, `.03` tall, `.04` wardrobe |
| `10.02` | Table | `.01` dining, `.02` desk |
| `10.03` | Seat | `.01` chair, `.02` stool |
| `10.04` | Bed | `.01` frame |
| `10.05` | Set | `.01` furniture set |

### 20 — Part

A single physical part, usually a panel.

| Code | Type | Variants |
|---|---|---|
| `20.01` | Panel | `.01` shelf, `.02` side, `.03` bottom, `.04` top, `.05` back, `.06` divider, `.07` drawer box part |
| `20.02` | Solid part | `.01` leg, `.02` rail |
| `20.03` | Glass part | `.01` glass shelf |

### 30 — Facade

A front that closes a cabinet. A facade is a container: it holds the panel, its edge banding and
the hardware mounted on it.

| Code | Type | Variants |
|---|---|---|
| `30.01` | Hinged door | `.01` overlay, `.02` inset |
| `30.02` | Drawer front | `.01` overlay, `.02` inset |
| `30.03` | Sliding door | `.01` sliding panel |
| `30.04` | Display door | `.01` glass frame |
| `30.05` | Lift-up front | `.01` flap |

### 40 — Hardware

A bought fitting. Hardware may be a leaf or a small container when it is made of several parts.

| Code | Type | Variants |
|---|---|---|
| `40.01` | Hinge | `.01` cup hinge, `.02` piano hinge |
| `40.02` | Runner | `.01` roller, `.02` ball-bearing, `.03` undermount |
| `40.03` | Handle | `.01` bar, `.02` knob, `.03` profile |
| `40.04` | Connector | `.01` cam and dowel, `.02` shelf pin |
| `40.05` | Lift mechanism | `.01` gas strut |
| `40.06` | Lock | `.01` cam lock |

### 50 — Appliance

A bought appliance that occupies space in or beside the furniture.

| Code | Type | Variants |
|---|---|---|
| `50.01` | Sink | `.01` undermount, `.02` inset |
| `50.02` | Hob | `.01` gas, `.02` induction |
| `50.03` | Oven | `.01` built-in |
| `50.04` | Extractor | `.01` built-in |
| `50.05` | Dishwasher | `.01` integrated |
| `50.06` | Refrigerator | `.01` built-in |
| `50.07` | Washer | `.01` integrated |

### 60 — Profile and long part

A linear part that is cut to length or a profile that frames other parts.

| Code | Type | Variants |
|---|---|---|
| `60.01` | Countertop | `.01` straight, `.02` shaped |
| `60.02` | Plinth | `.01` plinth board |
| `60.03` | Cornice | `.01` straight |
| `60.04` | Wall panel | `.01` wall board |
| `60.05` | Edge profile | `.01` end profile |
| `60.06` | Railing | `.01` hanging rail |
| `60.07` | Filler | `.01` filler strip |

New groups are reserved for later milestones and are added to this table before any command uses
them. A code that has been saved in a project is never renumbered; a withdrawn type stays in the
registry as retired and is no longer offered for assignment.

## 5. Assignment

- `furntype` is a reserved system property of every node (ADR 0034). Panels and containers both
  carry it.
- Setting a non-zero `furntype` on a container marks it as a **component**. Clearing it removes the
  marker unless the container also has `prototype.id` or `component = true`.
- `node.ungroup` refuses a component; `node.component.convert_to_group` clears the marker and the
  prototype keys, then the container behaves as an ordinary group.
- The UI assigns a type from the registry tree, never by typing digits, so a saved project cannot
  contain a code that the registry does not know. Loading an unknown code keeps the number and
  reports a diagnostic instead of rejecting the file.

A furniture type says what an object is. It does not say how it is made: material, finish,
dimensions, edge banding and hardware composition remain separate typed fields or properties.

## 6. Queries

The query layer matches on structural kind, a `FurnType` subtree or an exact code, and property
predicates. The same query feeds selection, reports and, later, CNC rules.

Examples:

```text
kind = container and furntype under 30        every facade
kind = panel and furntype = 20.01.01          every shelf in the project
kind = container and furntype under 40        all hardware, grouped by type
furntype = 40.02.03                           undermount runners only
furntype under 10 and property "finish" = oak products finished in oak
```

A report is a saved query plus a column set and grouping. A CNC rule later reads the same result,
so a report and a machining program cannot disagree about which parts are shelves.

## 7. What the code does not contain

- **Material and finish** — properties in the document, resolved through the catalogue.
- **Dimensions** — typed fields on a panel or properties on a container.
- **A prototype recipe** — `prototype.id` and `prototype.version` are separate properties that point
  at a user script and a database record, when those exist.
- **An instance number** — identifiers are document `Id`s, not furniture types.
- **A price or a supplier** — catalogue data, never a classification code.

## 8. Compatibility

FurniCraft does not import or reproduce any other program's classifier, including K3. The six-digit
shape is a local convention chosen because it is small, readable and queryable. Files saved by
FurniCraft carry only these codes, and a code means what this registry says it means.

## 9. Open questions

- Where the registry lives at runtime: compiled with `model`, or loaded from a versioned resource
  file so a workshop can add a private group without a rebuild.
- Whether retired codes keep their variants visible in the picker for editing an old project.
- Whether a component may carry more than one code (for example "hardware" and "hinge") or whether
  the tree already answers every report.
- How a user-script prototype declares the type it produces, and whether the type is fixed by the
  script or chosen per instance.
