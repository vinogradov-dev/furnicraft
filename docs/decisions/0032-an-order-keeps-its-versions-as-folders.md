# 0032. An order keeps its versions as numbered folders

- Status: Accepted
- Date: 2026-10-27

## Context

An order is the project file, the records about it and the files produced for it
([0031](0031-external-resources-keep-their-own-system-of-record.md)). The customer often considers
several variants — a different width, another front, a different material — while the workshop will
produce exactly one of them. Overwriting the files on each change would lose the variant the customer
approved and make a past production run unreproducible.

## Decision

- An **order** is a container on disk and a record in the order registry. It owns one or more
  **versions**.
- A **version** is a numbered folder inside the order that holds the complete set of files for that
  version: the project file, drawings, CNC programs and any other output. The folder is the unit of
  discussion, approval and production — not an individual file.
- The registry records the version's status, and **at most one version is marked as sent to
  production**. Marking it is an explicit, confirmed action in the order registry.
- A version folder is changed only while it is a draft. Once it is sent to production it is not
  edited; a further change creates the next version folder, so what the workshop received stays
  readable.
- The workshop works from one folder, the project file inside a version is the file the editor opens,
  and the order registry is the only place that knows which version is current and which was issued.
- The editor does not invent this layout: it asks the registry for an order and a version and opens
  the project file it names.

## Consequences

- Variants are cheap for the salon and safe for the workshop: each is a folder, and only the approved
  one is marked for production.
- A production run can always be traced back to the exact set of files that was sent, without a
  content-hash scheme or a version-control system behind it.
- Files are duplicated per version; for furniture projects this is acceptable and easy to explain.
- Save/load and a project store are prerequisites: the editor has to write into a version folder and
  the registry has to name it.
- The folder naming, the status values and the transition rules belong to the order registry's
  schema, not to the document format.

## Related

- [0031](0031-external-resources-keep-their-own-system-of-record.md) makes the registry the system of
  record for orders.
- `PROJECT.md` › Assistant (Faber); `plan/STATUS.md` › Open questions.
