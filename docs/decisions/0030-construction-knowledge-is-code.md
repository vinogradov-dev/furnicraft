# 0030. Construction knowledge is code, not prose

- Status: Accepted
- Date: 2026-10-27

## Context

The expected conversation is short: "build a hinged wardrobe 600 wide, 2200 high, 500 deep", then
"make it wider", then "change the material". For this to work, the system has to know what a
wardrobe is made of: which panels, how they are placed, what the gaps, banding and hardware are.

Three sources of that knowledge were possible: the language model's general knowledge, retrieval
from prose documents, or deterministic constructors in the code. General knowledge is unreliable and
differs from every workshop. Retrieval cannot be validated as geometry. Only a deterministic builder
can be tested, versioned and shown to the user.

## Decision

- A **furniture type** is a deterministic builder in `model` and `param`, arriving with M6: named
  dimensions, a resolved set of panels, placements, banding and hardware, with invariants and tests.
  Its output is derived from the document, never stored as a hand-built part list.
- The document stores the type id and its parameters as the M6 schema decides. This record does not
  invent that schema; it fixes that the builder is code and its result is reproducible.
- A **workshop profile** holds the technology defaults and limits: board thickness, door gaps, shelf
  pitch, banding defaults, sheet size, tolerance. It is data the user edits once; the builder reads
  it, and the model only passes parameters. Where the profile lives — project, application settings
  or the external resource system — is decided with the M6 model.
- The **type catalogue shown to the model is generated from the registry**, together with its
  parameter schema. Documentation is a view of the code, so it cannot drift from the builder.
- **Prose knowledge is advisory and deferred.** Catalogues, norms, past jobs and notes may later be
  searched and handed to the model as context, but they only influence which operation and which
  parameters are chosen; they never produce geometry directly.
- **Vision is not part of this loop.** Verification of a build is numeric: dimensions, part counts,
  bounds, diagnostics.

## Consequences

- "Build a wardrobe" becomes one high-level command plus parameter changes, instead of dozens of
  panel placements; the same builder serves the UI, the CLI and Faber.
- The useful assistant depends on M6. Before it, only a scripted, low-level demonstration is
  meaningful. That is an ordering fact, not a shortcut to take.
- Adding or changing a furniture type is a code change with tests, reviewed like any other — not a
  prompt edit, and not a document schema edit made in passing.
- Workshop standards live in one editable place instead of in prompts. A wrong default is a profile
  problem, not a model problem.
- Retrieval can be added later without touching the geometry path, which keeps the assistant
  testable with and without it.

## Related

- [0028](0028-the-assistant-is-a-client-of-the-command-registry.md) makes the assistant a client of
  these builders through commands.
- `PROJECT.md` › `Dimension`, `Assembly`; the M6 milestone.
- `AGENTS.md` §4.9 (do not invent the domain).
