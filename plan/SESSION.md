# Session

```text
task: V3
goal: the frame joins the box editor: its grips edit the envelope and the drawn gesture creates it

steps:
  [ ] the frame's grips edit the envelope through product.envelope.set

now: the frame's grips edit the envelope through product.envelope.set
blocked: nothing
run: cargo test -p furnicraft box_gizmo V3
resume: read the first `files:` path and its callers
```
