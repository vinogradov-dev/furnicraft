# Session

```text
task: U28
goal: the 18 drawn icons carry tone and one axonometry, and a pixel test holds them to it

steps:
  [x] plan read, session started
  [x] allow fill-opacity and publish the projection, footprint and tone in the docs
  [x] redraw the 18 drawn SVGs at ink 3..21 and coverage 20..45 %
  [x] coverage and footprint test read off the rasterised alpha
  [ ] cargo xtask ci green, icons checked by eye in both themes

now: run the full gate and review the icons in the real toolbar
blocked: nothing
run: cargo xtask ci
resume: cargo xtask ci, then update the plan and commit
map: icons — assets/icons/STYLE_GUIDE.md, crates/ui/src/icons.rs
decisions: coverage is alpha-weighted ink over the fixed 3..21 footprint (324 units2)
```
