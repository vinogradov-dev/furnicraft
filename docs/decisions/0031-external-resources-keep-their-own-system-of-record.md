# 0031. External resources keep their own system of record

- Status: Accepted
- Date: 2026-10-27

## Context

Faber must manage more than the editor. The material catalogue and the order registry are separate
programs with their own databases and their own UIs. A person uses those interfaces; the assistant
has to work the same way, not through a side door into the database.

Orders are not just rows: an order is the project file, the records about it and the files produced
for it — drawings, CNC programs and later purchase data. Those pieces have to stay consistent while
several systems are involved.

## Decision

- **The API is the boundary.** Every resource is reached through the same API its own UI uses,
  locally a CLI or a socket and remotely an HTTP endpoint. No component reads or writes another
  system's database, and Faber never does.
- **Ports in the core, adapters at the composition boundary.** Each resource is a port with typed
  operations; the adapter lives where the shell wires it, as `kernel-truck` does for the kernel. The
  core never learns the transport, the SQL or the vendor.
- **One owner per datum.** The editor keeps its catalogue as a read-through cache with stable ids
  and fallback values ([0005](0005-material-catalogue.md)) and reports staleness; it never writes
  material records. The order registry owns orders; the editor owns the project file.
- **An order is a bundle**: the project file, the database records and the files on disk. It must
  pin the project content it was issued from, so a later edit cannot silently change what was sent
  to production; how the pin is represented — snapshot copy, content hash or revisions — is decided
  with the order schema.
- **Multi-system operations are sagas.** Validate, act, record, and compensate on failure; steps are
  idempotent and every external call is audited. There are no distributed transactions, and no
  silent divergence: an unavailable resource produces a typed error the assistant must report.
- **External side effects need a person.** Placing a material order, issuing an order or sending
  anything outside the machine is never autonomous by default; it requires an explicit confirmation
  and an idempotency key, so a retry cannot duplicate it.
- **One path for every client.** Any resource operation Faber can perform is also reachable from the
  UI or a CLI. A capability that exists only in the agent cannot be tested or explained.

## Consequences

- The assistant can be useful across the workshop without owning anyone's data, and the systems can
  evolve independently behind their APIs.
- Publishing an order becomes an explicit, confirmed action with a traceable result; a failed step
  is reported as a failure, not a guess.
- The workshop must run those APIs (or their CLIs) and keep them reachable; degraded mode is a
  normal state the UI and Faber both have to show.
- The catalogue may be stale. Every operation that depends on it resolves ids through the cache and
  surfaces the staleness instead of trusting a name.
- Order pinning makes save/load and project revisions a prerequisite for the order workflow rather
  than a later polish item.
- The resource adapters are extra crates and configuration; they stay outside the core and can be
  omitted from a build.

## Related

- [0028](0028-the-assistant-is-a-client-of-the-command-registry.md) is the client rule;
  [0005](0005-material-catalogue.md) established the catalogue as an external, session-scoped cache.
- `PROJECT.md` › `Material`, `Hardware`; `docs/domain/materials.md`.
- `AGENTS.md` §4.8 (dependencies), §4.5 (no format assumptions).
