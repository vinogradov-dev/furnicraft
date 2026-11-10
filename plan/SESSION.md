# Session

```text
task: P4
goal: one command prints a whole task — the plan, the notes, the code facts — for a human and an agent

steps:
  [ ] parse the labelled fields and the paths of one backlog entry
  [ ] read plan/notes/<ID>.md and gate its id, staleness and budget
  [ ] render one task for a human; add `dashboard --task <ID>`
  [ ] add the xtask layer: line counts, last commit, ADR titles, `--md`, `--paths`
  [ ] point status, the templates, AGENTS.md and plan/refactor.md at the new command
  [ ] cargo xtask ci and the commit

now: the parse layer in `state-contract/src/parse.rs`
blocked: nothing
run: cargo test -p state-contract
resume: add `Task::fields`/`Task::paths` in `state-contract/src/parse.rs` and run the suite
decisions: notes live in `plan/notes/<ID>.md`, one file per task, read only by `task`, never by
  `status`; the human form stays in `state-contract` (box styling), the code facts and the
  `--md`/`--paths` forms live in `xtask`, because `docs/decisions/` and `crates/` are product
  knowledge the generic gate must not learn
```
