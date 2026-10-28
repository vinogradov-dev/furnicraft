# 0034. Nodes carry typed properties; containers are one kind

- Status: Accepted; supersedes 0021 and 0030
- Date: 2026-10-28

## Context

ADR 0021 split the tree into two container kinds: an **assembly** that carries component identity
and a **group** that is disposable organisation. That was a reasonable split while a component was
defined by its node kind, but it hardcodes one attribute — "is this a component?" — into the shape
of the data.

The product now needs more. A wardrobe, a hinge of three parts, a facade holding one panel and its
hardware, and an ordinary temporary group are all the same structure: a named transform with
children. What differs is the data attached to them: a furniture type, a prototype reference,
parameters, a finish, a supplier code, a note from a script. Making each family a separate node
struct would mean a code change and a schema version for every new attribute, and it would drift
back toward built-in product builders, which the owner rejected: product templates are user
scripts.

Two more facts force the same answer:

- reports and the future assistant must select objects by type and by property, not by name;
- prototype parameters are nested and differ per prototype, so they cannot be columns designed in
  advance.

## Decision

- **One container kind.** `Container` replaces `Assembly` and `Group`. It has an id, a name, a
  placement, a pivot, children and properties. The project root is a container. "Assembly" remains
  a word for a container that is marked as a component; it is not a separate type.
- **Every node has properties.** Panels and containers both carry a `Properties` collection — an
  ordered map from a key to a `PropertyValue`. The map is document data: it is serialized, copied
  with the node and restored by undo.
- **Values are typed.** `PropertyValue` covers `Bool`, `Integer`, `Length`, `Angle`, `Text`,
  `Choice`, `Reference`, `FurnType` and `Structured(serde_json::Value)`. Physical values use the
  domain units; a bare float is never a property value. `Structured` holds a nested prototype
  record exactly as a script wrote it and is rendered in the UI as fields driven by the prototype
  schema.
- **Intrinsic fields stay fields.** Name, placement, pivot, panel size, material and machining are
  not duplicated in the property bag. A property is data that has no dedicated column.
- **System keys are reserved.** `component`, `furntype`, `prototype.id`, `prototype.version` and
  `prototype.parameters` are written only through validated commands. User properties live in their
  own dotted namespaces. A reserved key with the wrong value type is rejected, not coerced.
- **A component is a marker, not a kind.** A container is a component when it carries a non-zero
  `furntype`, a `prototype.id`, or an explicit `component = true`. `node.ungroup` refuses a
  component and flattens an ordinary container as before. `node.component.convert_to_group` clears
  the marker and the prototype keys, after which the container can be ungrouped.
- **Every change is a command.** `node.property.set` and `node.property.clear` edit one node; a
  batch variant edits a selection as one revision and one undo entry. Container creation, grouping,
  ungrouping and conversion go through the `node.*` commands like any other mutation.
- **Schema 7 migrates old files.** A version 6 `assembly` becomes a `Container` with
  `component = true`; a version 6 `group` becomes a `Container` with no marker. No geometry,
  placement or pivot changes. The root keeps its zero pivot.
- **No automatic type inference.** Nothing guesses a furniture type from a name, a panel size or a
  script. A type is assigned explicitly or by a script through the property command.

## Consequences

- The tree has one container label with a component badge, and the inspector gains a properties
  card that works for panels and containers alike.
- "Assembly" survives in prose only; code, commands and files speak of a container and its
  properties. A tool may not infer meaning from the node enum.
- A new attribute is data, not a schema change. A new *type* of value still needs a model change,
  which is deliberate: a free-form string bag would move unit errors back into the document.
- Unknown property keys survive a load, so a file written by a newer build does not lose user data.
  An unknown value variant or an invalid system value is a readable error, never a silent guess.
- The ungroup rule now depends on a property. Clearing a marker is an explicit, undoable command,
  not a side effect of flattening.
- Product prototypes can arrive as user scripts without another model refactor: the document keeps
  the materialized geometry, the type, the prototype reference and its parameter record.

## Related

- [0021](0021-assemblies-are-components-groups-are-containers.md) is superseded: the two container
  kinds become one, and component identity moves into a property.
- [0030](0030-construction-knowledge-is-code.md) is superseded: construction knowledge is still
  deterministic, but product templates are user scripts rather than built-in builders.
- [0035](0035-furniture-types-form-a-numbered-tree.md) defines the numeric type stored in
  `furntype`.
- [0028](0028-the-assistant-is-a-client-of-the-command-registry.md) — the assistant edits
  properties through the same commands a person uses.
