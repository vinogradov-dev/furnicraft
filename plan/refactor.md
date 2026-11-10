# Design-rules refactor

Reference file for task **S7** and its subtasks (`S8`, `S9`, …). It holds the rules the refactor
follows, how to measure the tree, the audit procedure, the subtask template and the legibility test
that says a subtask is done. S7 reads all of it; a subtask reads §3–§8 and cites the rule ids it
obeys. `AGENTS.md` wins wherever this file disagrees with it, and this file leaves the plan in the
same commit that closes the last subtask.

Measured baseline 2026-11-10, kept in `plan/BACKLOG.md` S7 as F1–F7. Re-measure per §2 before
trusting a number; the numbers are evidence for the audit, the rules are the part that lasts.

## 1. Goal: a tree one human can hold in their head

The product is maintained by one person. Two properties make that possible, and nothing else in
this file matters if these two hold:

1. **Structure.** Every module has a place in the layer map and one reason to change; a reader
   finds the module that owns an operation from `plan/map.md` in at most two hops.
2. **Locality of change.** A change to X touches one source module, its tests and one plan row —
   not five files in four layers. Adding an attribute to a panel is the measuring stick: today it
   touches `model/panel`, `model/project/panel_cmds`, `engine/panel_cmds`, `engine/commands`,
   `engine/lib.rs`, `ui/panel_card` and often `ui/palette`; after the refactor it is three edits.

The acceptance test is not style, it is a stranger reading a module header and predicting where a
change goes (§8). "Clean code" here means: one operation per unit, one reason to change, no hidden
duplicate knowledge — not shorter lines.

## 2. How to measure

Production lines are the lines before the last `#[cfg(test)] mod tests` block; a test file that sits
beside its module counts as test lines. Size, methods and fields are read from the source, never
from memory:

```sh
# production lines per file (approximate: everything before the last test module)
for f in $(git ls-files 'crates/*/src/*.rs' 'apps/*/src/*.rs' | grep -v /tests/); do
  n=$(grep -n 'cfg(test)' "$f" | head -1 | cut -d: -f1); echo "${n:-$(wc -l <"$f")} $f"
done | sort -rn | head -20
```

Count public methods per `impl` and fields per struct with `grep -c '^\s\{4\}pub fn'` and
`grep -c '^\s\{4\}[a-z_]*:'` inside the type's range. A finding is real when it crosses a ceiling in
§3; below the ceiling it is a smell the audit may record but does not file.

## 3. Module boundaries

- **B1** One module, one operation or one value family. `geom/region.rs` becomes `rect`,
  `boolean` and `ring`; a new operation is a new module, not a new section of an old one.
- **B2** One reason to change, and the module header says it: `//! Changes when: …`. Two unrelated
  "changes when" in one header is the proof of an SRP breach, without further argument.
- **B3** `mod.rs` and `lib.rs` hold re-exports and the layer statement only; a `mod.rs` with logic
  is a finding.
- **B4** Size ceilings: a module is a finding at 1000 production lines, a function at 150, an
  `impl` at 800. Targets, not ceilings: 600, 60, 400. One **named algorithm** module may sit between
  600 and 1200 when its header states the algorithm, its reference and its refusals — the ceiling
  still applies above 1200, and an exception is a `plan/debt.md` line, never silence.
- **B5** A type with 12 or more fields is a finding unless it is pure document data or a
  composition root whose methods each touch one field; both cases say which in the module header.
  Behaviour plus many fields is the god object (F1, F2).
- **B6** 25 or more public items in one module is an ISP finding: split by area, or state in the
  header why the surface has to stay together.
- **B7** Names describe what is held, never where it is used: no `utils`, `helpers`, `common`,
  `misc`, `manager`, `shared`, `core`. A reader guesses the path from the name.
- **B8** A move changes no logic. A move and a fix never share a commit; a moved test keeps its
  assertions and changes only its path and names.

## 4. Data and behaviour (borrowed from SolidCraft, kept in FurniCraft's units)

- **D1** Geometry never lives inside document data. A document type holds `Length`, `Degrees`, ids
  and invariants; the maths over it is a free function or a trait **in the lower layer** (`geom`),
  as SolidCraft's `Seg2Geom for Seg2` does for its record type. This is the fix for F3, and the rule
  that keeps it fixed: `model` may call `geom`, never compute trigonometry itself.
- **D2** A geometric operation takes its tolerance as an argument or reads one named constant of its
  own layer, and its header states what the tolerance means; no magic `1e-6` in a call site.
- **D3** Units are explicit at the boundary and floats only inside it (`AGENTS.md` §4.3): `geom` in
  millimetres and radians, `model` and above in `Length`/`Degrees`, conversion once.
- **D4** A closed/open distinction that a caller can get wrong is a type, not a flag —
  `Ring`/`Path`, `PanelRect`, `CornerMove`. No `bool` parameter in a public function.
- **D5** Derived values are functions of their inputs: no cache inside a document type, no state a
  reader has to reconstruct by walking the tree (`AGENTS.md` §4.2).

## 5. Clean code rules the subtasks must satisfy

- **C1** A function does one thing; early return over nesting; nesting depth at most 3.
- **C2** At most four parameters; more is a named struct. No `&mut` parameters on a pure helper.
- **C3** A `match` arm longer than 15 lines is extracted into a named function.
- **C4** No `#[allow(...)]`, no new `unwrap`/`expect` outside tests, no dead code, no commented-out
  code, no `#[cfg]` graveyard. `cargo clippy -- -D warnings` is the floor, not the goal.
- **C5** Names are nouns for values and verbs for actions; a name that needs a comment to explain
  is renamed.
- **C6** Comments say why and what the contract is; they never narrate the code. A module header is
  a contract, a block comment is a reason.
- **C7** One source of truth per constant. The UI never holds a document default (F2: 600 × 400 mm
  belongs to the command), and a layer never re-declares a constant of the layer below.
- **C8** Errors are typed in the layer that raises them and carry what the user can act on; the UI
  maps them to sentences at the boundary (`AGENTS.md` §4.4, `PROJECT.md` §Error).
- **C9** No I/O, no clock, no global state in `geom`, `model` or `engine` command bodies; a pure
  function's inputs are its arguments.
- **C10** Every public item keeps a doc comment, and a crate's `//!` states its layer, its job and
  what it does not do.

## 6. Module documentation (borrowed from SolidCraft, extended)

Every module header written or moved by the refactor has, in this order:

- **H1** What the module holds, in one sentence.
- **H2** The contract: inputs, outputs, and the tolerance or precision that matters.
- **H3** What it refuses: the inputs that are an error rather than wrong output.
- **H4** For an algorithm: the method and its reference (a paper or a standard, as SolidCraft's
  `fill.rs` cites Piegl & Tiller).
- **H5** `//! Changes when: …` — the single reason (B2).
- **H6** What it does not do, when a reader would otherwise assume it does — "no self-intersection
  test", "display only, nothing measured reads this".

`plan/map.md` gets or updates the feature row in the same commit; an ADR that placed a rule is
amended when the rule moves between layers (0026, 0029, 0060, 0082 are in play).

## 7. Tests

- **T1** The existing suite passes **unchanged** through a move; only a path, a `use` line and a
  module name may change.
- **T2** A test module over roughly 300 lines moves to a sibling file (`#[path = "name_tests.rs"]
  mod name_tests;`), the convention SolidCraft uses, so the production size of a file stays
  readable. This is optional and never mixed into a logic commit.
- **T3** A regression test goes into the area's existing test module, never into a per-bug file.
- **T4** A split that changes no behaviour adds no test; it may add one **characterisation** test
  only where the moved code had none and the header claims a contract (H2, H3).
- **T5** `cargo xtask ci` green in the commit; report the production-line numbers before and after
  in the subtask entry, so the next reader sees that the split paid.

## 8. The legibility test (done means this, not "it compiles")

A subtask is done when all of these hold for the area it touched:

- **L1** From the operation's name, `plan/map.md` reaches the module in at most two hops.
- **L2** The crate `lib.rs` header and the touched module headers state layer, job, contract,
  refusals and "changes when".
- **L3** A new change of the same kind touches at most three source files (the locality stick, §1).
- **L4** No ceiling of §3 is crossed in the touched area, and every remaining exception is a
  `plan/debt.md` line with a reason.
- **L5** `git show --stat` reads as one concept and `git diff -M` shows the move as a rename.
- **L6** Reading only the crate header, the module header, the type doc and the public signatures
  is enough to explain the unit without the bodies.
- **L7** No comment in the touched files repeats what the code says; no name needs one.
- **L8** A stranger can answer "where would I add an attribute to a panel / a new draw tool / a new
  geometric operation?" by pointing at two paths and no others.

## 9. What to borrow, what not to

| Borrow from SolidCraft | Why |
|---|---|
| one module per geometric operation (`arcfit`, `fill`, `network`, `hlr`, `tess`, `section`) | an endless algorithm space decomposes by operation, not by entity |
| behaviour as traits/free functions over plain data (`Seg2Geom for Seg2`) | the document type cannot grow trigonometry; fixes F3 structurally |
| module header = method + reference + tolerance + refusals | a mini-ADR per module, which is what a maintainer needs |
| sibling `*_tests.rs` files | keeps production size honest and files short |
| 220 command modules, one per area, with parameter schemas | the answer to our `[engine]` "no parameter schema" debt |
| typestate for closed/open (`Ring`/`Path`) | misuse becomes a compile error |

| Do not copy | Why |
|---|---|
| a 14k-line `viewport.rs`, an 11k-line `prefs.rs` | their bottom layers are exemplary and their top layers are not; that is the failure we are fixing |
| `Exempt` binaries outside the layer rules | our L8 with forbidden edges is stricter and stays |
| two geometry kernels behind one `guard` | one port, one adapter; a second backend is an ADR, not a default |
| ADRs in an untracked `plan/` | our `docs/decisions/` stays in the repository |
| an L0 crate that also holds colours, paper sizes and title blocks | L0 is numbers and geometry; a drawing sheet is a domain object of a later layer |

| Keep from FurniCraft | Why |
|---|---|
| layer map with per-crate forbidden edges (`cargo xtask layers`) | the cheapest architecture test there is |
| commands with stable ids, a registry and undo | the only path into the document, and it is already honest |
| `Length`/`Degrees` at the domain edge | units are where most CAD bugs live |
| ADRs in the repository, `plan/map.md`, the plan gate | the "why" and the "where" survive the author |
| panic lints and no `unsafe` | a furniture file must never crash the editor |
| measuring production lines apart from tests | it is what makes F1–F7 visible at all |

## 10. The audit (S7 itself)

1. `cargo xtask ci` on the current tree: a green baseline, or stop and report.
2. Re-measure §2 and replace the F1–F7 numbers with the current ones, or mark them confirmed.
3. Re-read the sources that moved since filing (S6, U45, R1, the U-set — see the S7 entry).
4. For every finding: one verdict — mechanical split (B1, B4), extract the type or module (B5, B6),
   move down the layer arrow (D1), or no action with one line of why.
5. Order the verdicts by dependency (§12) and file one subtask entry per verdict using §11. A
   finding that needs a domain decision goes to `plan/questions.md` and is not filed.
6. A no-action finding goes to `plan/debt.md` under a new `[design]` tag, whose legend the audit
   extends, so a later session does not re-open it.
7. Close the audit commit with `plan/` changes only, and leave `plan/STATUS.md` naming S8.

## 11. Subtask template

```text
- `[ ]` **S<N>** <the operation, one line> — rules: B1, H4, T2.
  Why: <the measured finding, one sentence, with the number that crossed a ceiling>.
  Shape: <what moves where, and the production lines before -> after>.
  Done: <tests that must pass unchanged>; <map/ADR/header updates>; <L checks touched>.
  Commits: one move, plus one more only if a fix is forced.
  `files: <two or three paths that exist>`
```

A subtask is behaviour-preserving. A change that alters what the program does is an `X` task
instead, filed under `## Bugs`, and the refactor entry links to it.

## 12. Order and stopping rules

- **Order:** `geom` and `model` (no UI dependency) → `scene`, `render`, `param` → `ui::Editor` →
  the shell. A subtask that touches `main.rs`, `crates/ui/src/lib.rs` or `file_menu.rs` lands
  before U45, which moves those files into `Document`; U45 then re-plans against the smaller result.
- **Small:** a subtask is one sitting. A mechanical move above roughly 800 changed lines is split
  further; a 1600-line file is two subtasks, not one.
- **No speculation:** no empty module for a future operation, no rename for taste, no dependency
  addition, no prelude until two traits need one, no "while I am here".
- **No behaviour change:** if the split exposes a real bug, stop, file it, do not fix it inside the
  move.
- **Ask:** a finding that changes a domain rule, the file format, a command id or the crate map is a
  question for the owner, not a refactor.
