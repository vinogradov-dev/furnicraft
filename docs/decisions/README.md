# Architecture decisions

One file per decision, short: the situation, the decision, and what it costs us. Decisions are
superseded, never rewritten — add a new record and mark the old one `Superseded by NNNN`.

| # | Title | Status |
|---|---|---|
| [0001](0001-geometry-kernel.md) | Geometry kernel behind a port, implemented with truck | Accepted |
| [0002](0002-vendored-truck-shapeops.md) | Vendored `truck-shapeops` without the VTK stack | Accepted |
| [0003](0003-length-is-integer-micrometres.md) | Lengths are integer micrometres | Accepted |
| [0004](0004-panel-local-frame.md) | Panel-local frame, faces and topology ids | Accepted |

## Format

```text
# NNNN. Title

- Status: Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
What forced the decision.

## Decision
What we do, in the present tense.

## Consequences
What it buys, what it costs, and the condition under which it should be revisited.
```

The code and `AGENTS.md` stay the authority for rules; a record keeps the reason.
