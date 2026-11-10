# state-contract

A plan gate for agent-driven projects: three small files carry the state of the work, and one check
keeps them honest. Zero dependencies, standard library only.

## What

The contract is three files under `plan/`:

- `STATUS.md` — exactly one `STATE:` line (`milestone`, `phase`, `task`, `state`, `base`, `updated`),
  a `Where we are` section and a `Next` paragraph;
- `BACKLOG.md` — one entry per task with a stable id and a `files:` hint, in `## Now`, `## Next` and
  `## Done`;
- `SESSION.md` — the live note while a task is in progress: `task`, `goal`, `steps`, `now`, `blocked`,
  `run`.

`plan/notes/<ID>.md` is an optional fourth thing: the research note of one task. It carries what a
session measured and ruled out — file and line pointers, dead ends, choices — never status, a
`files:` hint or a commit, so it cannot contradict the plan. `task` prints it; nothing else reads it.

`state_contract::check(root)` parses them, cross-checks them and returns a `Summary` — including
whether the session looks interrupted — or every violation as a readable sentence. `state_contract::task(root, id, opts)` returns everything the plan
says about one task, framed for a human, and `state_contract::task_view(root, id)` returns the same
as data for a host that adds facts of its own (git, file sizes, decisions). `state_contract::status(root)` returns the block a task runner
prints at the start of a session: the state line, the head commit, the session note's
`task`/`now`/`blocked`/`run` while a task runs, and the current task with the next three. The check
also reads `README.md`, when the project has one, and rejects a status line or a milestone there: a
README describes the product, the state lives in `plan/STATUS.md`.

## Usage

`cargo run -p state-contract -- dashboard` shows the current state with source paths: every section
(`STATE`, `NOW`, `NEXT`, `SESSION`, `QUESTIONS`, `DEBT`) names the file it was read from; sections for
missing files are omitted. Flags: `--root <path>` (default `.`), `--width <n>` (default 80, longer
lines truncate with `…`), `--no-color` (ANSI is on only when stdout is a terminal), `--task <id>`
(expand one task in the same frame). A session that looks interrupted — `state=doing`, a
`plan/SESSION.md`, `base != HEAD` and open `[ ]` steps — carries a red `INTERRUPTED` marker in its
header.

`cargo run -p state-contract -- task <id>` is the whole task on one screen: the entry with its
fields, the `files:` hint, the `plan/map.md` rows that name those paths, the debt and questions that
name them (or all of them, with `--all`; the block says how many it left out), the note, the notes of
*other* tasks that name the same files, the commit that closed a done task, and the live session
fields while it is the current task. `cargo run -p state-contract -- task list` prints one line per
task; a host adds the filters it needs (`xtask` has `--section`, `--status`, `--phase`, `--grep`).
`--width` and `--no-color` apply as above.

### Entry fields

A backlog entry may carry labelled parts after its opening sentence. They are parsed in file order,
in any case, and the report prints them one by one:

```markdown
- `[ ]` **A3** what the user gets, in one sentence — rules: B1, B4.
  Why: the observation that makes it a task, with the numbers.
  Shape: what moves where, in which file.
  Done: how a reader knows it landed.
  Commits: one, or one per step.
  `files: crates/model/src/lib.rs, crates/model/src/contour.rs`
```

Only `Rules`, `Why`, `Shape`, `Done` and `Commits` are fields; `files:` stays the hint it always was,
and the opening sentence before the first label is the `lead` — the line a one-screen status block
shows.

## Writing the plan

The crate parses and renders; the writer is a host's, and it gets two pieces from here. `edit`
splices line ranges — one command owns the lines of its entry, every other byte of the file is
copied — and `check_text` validates a plan that is still in memory, so a command that would break
the plan can refuse before anything is written:

```rust
let backlog = state_contract::edit::set_marker(&backlog, "A3", state_contract::edit::DOING)?;
let plan = state_contract::PlanText {
    status: &status,
    backlog: &backlog,
    session: Some(&session),
    notes: None,
};
state_contract::check_text(root, &plan, &git)?; // the caller writes only after this returns `Ok`
```

The prose under `## Next` is the one part of `plan/STATUS.md` a command has to own, because the gate
requires it to name the current task: `edit::set_section` replaces a whole section body, and a close
that changes the current task takes the new paragraph with it.

`FurniCraft`'s `cargo xtask task add|start|set|step|note|close|reopen|drop` and `cargo xtask state
set` are the reference host: they take `--dry-run`, print the diff, hold `plan/.lock` while they
write and land each file through a temp file and a rename.

## Why

An agent starts with no memory of the previous session. The plan files are that memory; the gate is
what stops them from lying. A `STATE` pointing at a task that is not in `## Now` sends the next
session to the wrong place, a dead `files:` hint sends it to a file that does not exist, and a
session note without `now:` turns a resume into a rewrite.

## Install

1. Add the crate to the tool that runs your checks (a path, git or registry dependency):

   ```sh
   cargo add --path ../state-contract state-contract
   ```

2. Copy the templates into your plan directory and replace the placeholders — `{{PROJECT}}`,
   `{{MILESTONE}}`, `{{TASK_ID}}`, `{{DATE}}`, `{{TASK_COMMAND}}` (for example `cargo xtask task`):

   ```sh
   mkdir -p plan
   cp ../state-contract/templates/STATUS.md ../state-contract/templates/BACKLOG.md plan/
   cp ../state-contract/templates/SESSION.template.md plan/
   mkdir -p plan/notes && cp ../state-contract/templates/NOTE.md plan/notes/{{TASK_ID}}.md
   ```

3. Call `state_contract::check` from CI, `state_contract::status` from your task runner and
   `state_contract::task_view` where you print a task in full, then paste
   `templates/AGENTS.section.md` into the instructions your agent reads:

   ```rust
   let workspace = Path::new(".");
   match state_contract::status(workspace) {
       Ok(block) => print!("{block}"),
       Err(violations) => return Err(violations),
   }
   ```

## Checks

| Area | Fails when |
|---|---|
| state line | no `STATE:` line, a missing `milestone`/`task`/`state`/`base`/`updated` field, more than one `STATE:` line, or an indented one |
| state values | `state` is neither `idle` nor `doing`; `task=` names a task the backlog does not have, one that is done, one that sits outside `## Now`, or one that is not the first unfinished task in `## Now`; `updated=` is not a `YYYY-MM-DD` calendar date |
| state history | `updated=` is earlier than the date of the `base` commit |
| backlog shape | no `## Now`/`## Done`; a duplicated id; `[x]` outside `## Done`; a `## Done` entry without `[x]`; a task line without a `**bold**` id |
| files hints | an unfinished task without a `files:` hint, or a hint naming a path that does not exist |
| session note | `doing` without exactly one `[~]` task in `## Now`, without a `plan/SESSION.md`, or with a note about another task; a note missing `task`, `goal`, `now` or `run`; `idle` with a note or a `[~]` task |
| notes | a `plan/notes/` file that is not `<task id>.md`, one named after a task the backlog does not have, an empty note, or a note over 150 lines |
| next paragraph | the `## Next` section of `plan/STATUS.md` names another task than `STATE: task=`, or names none while the section exists |
| feature map | a `plan/map.md` row names a path that does not exist |
| debt tags | a `plan/debt.md` entry carries a tag the `Tags:` line does not declare, or tags without a `Tags:` line |
| budget | `STATUS.md` over 70 lines, `SESSION.md` over 50 — a warning nobody reads is not a check |
| history | a done task without a commit hash, or a hash that is not a commit in the repository |
| base | `base` is not a commit, or is not an ancestor of `HEAD`; `HEAD` itself is legal while the plan edit is uncommitted |
| readme | `README.md` carries a `Status:` line or an `M<digit>` milestone: a README describes the product, the state lives in `plan/STATUS.md` |

The `files:` hint may wrap onto an indented continuation line, and a task marker may be wrapped in
backticks for rendering; both forms parse.

## Extending

A check is a pure function that returns `Vec<String>`, called from `analyze` in `src/validate.rs`;
add it there and give it a test next to the others. A check that needs the filesystem or git belongs
in `check_with` and takes a `&dyn Git` (`src/git.rs`) so its test needs no repository: implement the
trait for a fake with a `head`, a commit list and an ancestor list.

## Used by

FurniCraft — a native cabinet-furniture editor in Rust — is the first consumer: `cargo xtask plan` calls
`state_contract::check`, `cargo xtask status` calls `state_contract::status`, `cargo xtask task` prints
the report plus the facts only a checkout has (production and test lines, the last commit, the ADR
titles), and its `AGENTS.md` §1.1 points at `templates/AGENTS.section.md`.

## License

MIT OR Apache-2.0.
