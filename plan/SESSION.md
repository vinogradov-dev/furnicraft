# Session

task: X23
goal: every close of a draft is one operation — close and commit — whatever asked for it, and the
  ribbon never leaves a ring as a draft with a rubber band on screen

steps:
  [x] the plan files carry X23 and the session
  [x] one `close_draft` in the shell; the ribbon and the status bar call it, the gizmo reports a close
  [x] `Editor::close_and_commit_drawing` reopens a ring whose commit was refused
  [x] a closed draft drops the cached preview
  [x] the floating strip blocks the pointer over its whole rectangle
  [x] `cargo xtask ci` green, the fix committed as ed0e8af
  [ ] the manual check of the three close buttons, then the plan close

now: the owner looks at the three close entry points in the running app
blocked: the manual check needs a pointer, which no tool in this session can drive
run: cargo test -p furnicraft-ui -p furnicraft --lib --tests
resume: on a confirmed manual check, mark X23 `[x]` with ed0e8af, rewrite STATUS, delete this file
map: `apps/furnicraft/src/main.rs` — the ribbon and the status bar of the shell
decisions: the close belongs to the shell because only it owns the work plane, the gizmo reset and
  `sync_scene`; `crates/ui` reports the intent like it already does for the view actions; the strip's
  blocker senses a drag as well as a click, because a click-only widget leaves the drag-sensing
  viewport under it hovered; the refused close is reached in a test through a project whose revision
  counter is full, the only refusal the public editor surface can produce
