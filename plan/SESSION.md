# Session

```text
task: X2
goal: a legal ring of any size becomes a solid instead of being refused for its point count

steps:
  [x] the chord tolerance is the finer of 0.05 mm and 1e-4 of the shape's bounding diagonal
  [x] a ring that still overflows the cap is re-sampled coarser, bounded, before it is refused
  [x] band strips, panel cores, cutouts and constructive bodies share that one sampler
  [x] tests: the reported band becomes a mesh, the chord grows and keeps its floor, the search converges
  [x] ADR 0062 and the decision index
  [ ] plan close: BACKLOG, STATUS, map, questions and debt, then cargo xtask ci

now: update plan/*, then run cargo xtask ci and commit
blocked: nothing
run: cargo test -p furnicraft-param --lib profile
resume: run cargo xtask ci, then commit and close X2 in the plan
map: plan/map.md — Contour to profile conversion; crates/param/src/profile.rs (ADR 0062)
decisions: the chord follows the shape and a bounded search coarsens it further; the cap stays 1024 and nothing is truncated silently
```
