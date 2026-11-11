# Session

```text
task: X34
goal: origin and pivot are separate concepts: origin stays implicit in Placement::translation, pivot becomes PivotLocal, rebase_origin preserves pivot_world, a panel's pivot stays derived zero, and the card and schema say so

steps:
  [ ] P1: implement rebase_origin and prove origin/pivot separation with invariants 1-6, 10-12
  [ ] P0 landed in 1dab333: origin vocabulary, PivotLocal, old wire alias

now: P1: implement rebase_origin and prove origin/pivot separation with invariants 1-6, 10-12
blocked: nothing
run: cargo test -p furnicraft-model X34
resume: read the first `files:` path and its callers
```
