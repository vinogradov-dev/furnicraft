# 0096. Before the first release the data format has no past

- Status: Accepted; amends the compatibility clause of `AGENTS.md` §4.5; retires the migration
  clauses of 0006, 0014, 0026, 0034, 0040, 0073, 0074 and 0088 until 1.0
- Date: 2026-11-12

## Context

The document schema is at version 20 (`crates/model/src/project/mod.rs`). Getting there produced a
repository full of compatibility machinery: nine golden files `scene-v7.fcs` … `scene-v20.fcs`,
two migration passes that walk the tree, a set of `serde` aliases for renamed fields, a readable
error for a file from a newer build, and a section in each of `docs/domain/document.md` and
`docs/domain/scene-file.md` that explains how old layouts become the current one.

None of that has ever served a user. The scene format has not shipped — `plan/debt.md` says so
explicitly, and the pre-release rename is still ahead. Every file it protects was written by a
previous build of this same tree and is reproducible from a fixture. Meanwhile the machinery has a
real cost: each schema change touches the migration pass, the aliases, the goldens and two
documents, and a wrong alias silently reinterprets data that no compatibility promise covers.

The owner's rule, stated 2026-11-12: before the first release, do not carry backward compatibility
of the data format. If a mechanism for it exists, remove it — unused code is garbage.

## Decision

**Before `1.0` the format has no past.**

- A schema bump changes the layout and deletes the old loader in the same change. No migration pass,
  no `serde` alias purely for an older field name, no golden file for an older schema.
- The format keeps one `schema_version` number. A file whose number is not the current one is a
  readable error that says which number the file carries and which one this build reads; a file from
  a newer build is the same error, not a separate code path.
- One golden file per current schema stays. When the schema moves, the file is re-generated and the
  old one is deleted. It proves the writer and the reader agree with each other *now*, which is all a
  golden file can honestly prove before there is a released format.
- `#[serde(default)]` stays only where the field is genuinely optional in the current format. A
  default that exists so a previous layout can load is removed with the migration.
- Documents already saved by a development build are disposable. A session that needs one opens it
  with the build that wrote it, or re-creates it from a fixture.

**At `1.0` this decision flips, and the flip is its own record.** From the first released format, the
opposite rule returns: older files keep loading, a bump ships a migration, new fields carry
`#[serde(default)]`, unknown sections are preserved, and a checklist item covers it. The day the
first release is cut, this ADR is superseded by one that restores compatibility — it does not linger
as a standing exemption.

The project rule itself is amended in `AGENTS.md` §4.5 so a later session does not have to find this
record to know which rule is live.

## Consequences

- The model loses two tree-walking migration passes, its migration tests, the older golden files and
  the aliases; the domain documents lose their "how version N became version N+1" sections. The
  schema number stays as a single sentinel and the newer-file error collapses into the same sentence.
- A developer who wants to open yesterday's scratch file cannot. That is the price and it is small:
  scratch files are disposable, and fixtures are in the tree.
- The next change that renames a stored field becomes one commit smaller: rename, bump the number,
  regenerate the golden file. That is what makes the class rename of 0094 cheap.
- The rule has a hard edge: the moment a build is handed to anyone outside the project with a promise
  that their file survives, `1.0` compatibility starts, whether or not the version number says so.
  Compatibility promises follow released users, not version numbers — so `1.0` must be declared, not
  drifted into.
- The removed machinery is not archived in the tree; `git log` holds it if a migration is ever needed
  as a historical reference for a file that somehow matters.

## Related

- [0038](0038-the-scene-file-lives-in-its-own-crate.md) — the document owns its schema and
  validation; this record removes its migration duty until 1.0.
- [0094](0094-a-class-registry-gives-the-machine-its-semantics.md) — the rename that spends the
  freedom this record buys.
- [`docs/domain/scene-file.md`](../domain/scene-file.md) — the envelope and its single version.
- [`docs/domain/document.md`](../domain/document.md) — the document schema.
