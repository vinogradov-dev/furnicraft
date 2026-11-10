# 0085. The plan is markdown, written through one tool — not a database

- Status: Accepted
- Date: 2026-11-10

## Context

The plan lives in `plan/*.md`: 2425 lines, 144 tasks, 194 commits that touched `plan/BACKLOG.md`. A
session reads it with `cargo xtask task` (P4) but writes it by hand — the entry, the `[~]` marker,
`STATE:`, `SESSION.md` and the delete at the end of a task — and every hand edit is a chance to break
the ordering and cross-file rules the gate enforces.

Two changes were on the table: make `state-contract` a full CRUD command line so an agent never
touches the files, and move the task data into SQLite (or another small embedded store).

## Decision

- **Markdown stays the source of truth.** The files are the plan: `plan/STATUS.md`,
  `plan/BACKLOG.md`, `plan/SESSION.md`, `plan/notes/<ID>.md`, and the reference files beside them.
- **The tool is the writer an agent uses.** `cargo xtask task …` and `cargo xtask state …` build the
  new text, validate it with the gate in memory, and write it through a temp file and a rename. A
  command splices the lines it owns; the rest of the file stays byte-identical, so the diff of a plan
  commit stays readable. Every verb takes `--dry-run`. The owner may still edit a file by hand — the
  format is unchanged, and the gate catches what a hand edit breaks.
- **No database.** SQLite in Rust means `rusqlite` → `libsqlite3-sys`, which compiles `sqlite3.c`:
  a C dependency, forbidden by `AGENTS.md` §2 and §4.8. A binary store of any kind costs more than it
  buys:
  - git *is* the plan's database: `STATE: base=` is a commit, a done entry carries its commit hash,
    and the task report reads `git log` per file — a blob would kill `git diff`, `git blame` and the
    commit-per-operation cadence (§6.1);
  - three branches exist (`dev`, `main`, `qwen-dev`); a markdown conflict is resolved line by line,
    a store file is not;
  - the plan is read by the owner in an editor, by `rg`, by GitHub and by a second agent without our
    binary — a store makes the tool mandatory for reading;
  - `state-contract` is a zero-dependency, reusable plan gate; a store ties it to a schema, a file
    format and migrations;
  - there is no load to optimise: 144 tasks parse in milliseconds. The cost was always the agent's
    reading and hand-writing, which a command line removes without changing where the bytes live.

## Consequences

The writer is the risky half of the tool, so it carries the guarantees above and a test for each
refusal (a command that cannot produce a valid plan writes nothing). `plan/.lock` keeps two writers
apart, which is enough for one agent per branch.

Revisit this record only if one of these becomes true: several tasks write the plan concurrently from
inside the application (Faber), or `plan/` grows past the point where parsing is visible. Either way
the answer starts with a derived, git-implemented index, not with a second source of truth.
