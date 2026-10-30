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

`state_contract::check(root)` parses them, cross-checks them and returns a `Summary`, or every
violation as a readable sentence. `state_contract::status(root)` returns the block a task runner
prints at the start of a session: the state line, the current task and the next three. The check also
reads `README.md`, when the project has one, and rejects a status line or a milestone there: a README
describes the product, the state lives in `plan/STATUS.md`.

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
   `{{MILESTONE}}`, `{{TASK_ID}}`, `{{DATE}}`:

   ```sh
   mkdir -p plan
   cp ../state-contract/templates/STATUS.md ../state-contract/templates/BACKLOG.md plan/
   cp ../state-contract/templates/SESSION.template.md plan/
   ```

3. Call `state_contract::check` from CI and `state_contract::status` from your task runner, then
   paste `templates/AGENTS.section.md` into the instructions your agent reads:

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
| state values | `state` is neither `idle` nor `doing`; `task=` names a task the backlog does not have, one that is done, or one that sits outside `## Now` |
| backlog shape | no `## Now`/`## Done`; a duplicated id; `[x]` outside `## Done`; a `## Done` entry without `[x]`; a task line without a `**bold**` id |
| files hints | an unfinished task without a `files:` hint, or a hint naming a path that does not exist |
| session note | `doing` without exactly one `[~]` task in `## Now`, without a `plan/SESSION.md`, or with a note about another task; a note missing `task`, `goal`, `now` or `run`; `idle` with a note or a `[~]` task |
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
`state_contract::check`, `cargo xtask status` calls `state_contract::status`, and its `AGENTS.md` §1.1
points at `templates/AGENTS.section.md`.

## License

MIT OR Apache-2.0.
