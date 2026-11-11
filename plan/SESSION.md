# Session

```text
task: U52
goal: the UI scale is a scrubbable number with presets: the Settings slider becomes a numeric field that changes while the pointer is held and moved left or right, with the cursor hidden for the duration and only the horizontal delta read, plus buttons for 75, 100, 125, 150, 175 and 200. The same *hide the pointer, read the delta* rule applies to every numeric field that scrubs on a horizontal drag, so a value that resizes its own widget — the scale is the worst case — no longer chases the cursor. A typed value is clamped to the offered range and applied through `ctx.set_zoom_factor`; a preset applies exactly and draws as selected. Tests: a drag reads the delta and never the absolute pointer position; the value does not jump when the widget changes size under it; a preset sets its exact percent; a typed value outside the range clamps. Session notes: `ui_scale::show` is the only scale control; `Response::drag_delta` or `ui.input(|i| i.pointer.delta())` is the input to read, and `CursorIcon::None` hides the pointer. Find every scrubbing numeric field through `egui::DragValue` in `crates/ui/src`.

steps:
  [x] scrub field, scale presets, tests and the looked-at screenshot

now: scrub field, scale presets, tests and the looked-at screenshot
blocked: nothing
run: cargo xtask task close U52
resume: commit the change, then close U52 with the hash
```
