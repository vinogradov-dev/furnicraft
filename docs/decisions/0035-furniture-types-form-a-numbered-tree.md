# 0035. Furniture types form a numbered tree

- Status: Accepted
- Date: 2026-10-28

## Context

Names are for people. A report that must collect every shelf, every facade or every hinge cannot
depend on what a user called a node, and a script that assembles a cabinet must mark its parts in a
way that survives a rename, a duplicate and a save.

A string tag would need a convention that every script and report re-implements. A free-form
property would be worse: "shelf", "Shelf" and "полка" would be three answers. A code with a fixed
shape can be validated, stored compactly and queried by subtree.

A classifier from another program was available as an example. It mixes several independent
questions into one code — family, shape, and even a second independent classification after a colon
— so a query cannot rely on the digits alone and a report has to know special cases. Copying it
would also bind FurniCraft to a foreign registry it does not control.

## Decision

- **Six digits, three levels.** A furniture type is `GG.TT.VV`: group, type, variant, stored as an
  integer from `0` to `999_999`. `000000` means unclassified and is the default.
- **Strict parent tree.** A variant's parent is its type with `VV = 00`; a type's parent is its
  group with `TT = 00`; a group's parent is `000000`. There are no sibling ranges, no aliases and
  no second colon-separated part.
- **Zero suffix is the parent.** A query on `30` matches every facade; a query on `30.01` matches
  every hinged door. Matching is prefix-based on the integer, which is why the digits are positional.
- **Codes are stable and opaque.** A code is assigned once and never reused. Labels and the
  hierarchy are registry data; commands and reports match numbers, so a translation or a rename
  does not touch a saved project.
- **The classifier answers "what is it", nothing else.** Material, finish, dimensions, price,
  supplier and placement stay separate fields or properties. A prototype is a separate reference
  (`prototype.id`, `prototype.version`) with a structured parameter record.
- **Assignment is explicit.** `furntype` is a reserved system property on every node. A non-zero
  value on a container marks it as a component ([0034](0034-nodes-carry-typed-properties.md)).
  Nothing infers a type from geometry, a name or a prototype.
- **Unknown codes are preserved.** Loading a project with a code the registry does not know keeps
  the number and reports a diagnostic; it does not reject the file or drop the property.
- **The registry starts small.** `docs/domain/furntypes.md` lists the groups and types this
  milestone supports. New codes are added there before a command or a report uses them.
- **No compatibility with K3.** FurniCraft neither imports nor reproduces another program's codes
  or tables. The six-digit scheme is local, and a FurniCraft file means only what FurniCraft's
  registry says.

## Consequences

- Selection, reports and later CNC rules share one query language over kind, type subtree and
  property predicates; a report and a machining program cannot disagree about which panels are
  shelves.
- Adding a furniture type is a registry entry, not a code change in the query layer.
- The registry will grow slowly and deliberately; a type that a workshop needs but the registry
  lacks is visible as unclassified instead of being smuggled into a free-form string.
- Two objects of the same type are still distinguished by their `Id`, name, placement and
  properties, so classification cannot become an identity.
- The scheme is not portable to another program without a mapping table, which is the price of not
  inheriting a foreign registry.

## Related

- [0034](0034-nodes-carry-typed-properties.md) — `furntype` is one of the reserved system
  properties, and a non-zero value marks a container as a component.
- [`docs/domain/furntypes.md`](../domain/furntypes.md) — the registry and query examples.
- [0028](0028-the-assistant-is-a-client-of-the-command-registry.md) — the assistant selects and
  edits objects through the same queries and commands.
