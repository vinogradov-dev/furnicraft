# Session

```text
task: X34
goal: origin and pivot are separate concepts: origin stays implicit in Placement::translation, pivot becomes PivotLocal, rebase_origin preserves pivot_world, a panel's pivot stays derived zero, and the card and schema say so

steps:
  [ ] P0: rename CreationPivot/creation_pivot/rebase_at_anchor, fold add_object_with_pivot into add_object, rename Pivot to PivotLocal and the field to pivot_local, add origin accessors

now: P0: rename CreationPivot/creation_pivot/rebase_at_anchor, fold add_object_with_pivot into add_object, rename Pivot to PivotLocal and the field to pivot_local, add origin accessors
blocked: nothing
run: cargo test -p furnicraft-model X34
resume: read the first `files:` path and its callers
```
