# Pre-release product rename

Reference file for task **R1**. It records what the product name touches today, what a rename
costs, and the exact procedure that was measured on this tree. Read it only when R1 is taken, or
when a change adds a new place the name appears.

Measured on 2026-11-07 at commit `0b568c3` (`dev`). Re-measure with the commands in §3 before
trusting the numbers; the procedure itself does not go stale as fast as the counts.

R1 waits for the release, not for its turn: it sits under `## Next` in `plan/BACKLOG.md`, out of the
work order, and the `[release]` entry in `plan/debt.md` is the reminder. Take it back with
`cargo xtask task take R1` when the release is planned.

## 1. Why the name is in the tree at all

Three different roles, with three different prices. Confusing them is what makes a rename look
expensive.

1. **Crate identifiers.** `furnicraft-geom`, `use furnicraft_geom::DVec3`. This is a naming
   convention for workspace uniqueness, not a requirement. The project publishes nothing and has
   no `repository`/`publish` key in any `Cargo.toml`, so nothing outside the repo reads these names.
2. **Data identity on disk.** `FORMAT = "furnicraft.scene"` inside every `.fcs`, and the settings
   directory (`~/.config/furnicraft`, `%APPDATA%\FurniCraft`,
   `~/Library/Application Support/FurniCraft`). This is the only part that is a real contract.
3. **Words on the screen.** Window title, header mark, Home heading. Cheap: `theme::PRODUCT_NAME`
   already exists, it is just not used everywhere.

Everything else — 20 `wgpu` debug labels in `crates/render/src/lib.rs`, `egui::Id` keys in
`crates/ui/src/icons.rs`, `//!` comments, prose in `docs/` — has no runtime meaning.

The name never reached the domain: no command id, no document field, no JSON key of the document
carries it. A rename therefore cannot corrupt a document beyond the envelope's `format` string.

## 2. Decisions that shape the work

| Question | Answer recorded here |
|---|---|
| Is there a `apps/` level above the single app? | No reason. `app/` at the workspace root works: `members = ["crates/*", "app", "xtask", "state-contract"]`, `default-members = ["crates/*", "app"]`. Verified: builds, 1140 tests pass, `cargo xtask layers` ok. |
| Prefix on crate names, or bare `geom`/`model`? | Bare names work and read best: `use model::Project`. All of them are taken on crates.io (`geom`, `model`, `ui`, `render`, `mesh`, `param`, `units`, `kernel`, `engine`, `testkit`, `project`, `app`), which matters only if the crates are ever published; `scene` and `state-contract` are free. Keep the publish escape hatch: a prefix can be added later with `package = "..."` aliasing without touching a single `use`. |
| Would `app-geom` / `app_model` be better than bare names? | It is a valid second choice with the same mechanical cost: `app-geom` is used as `app_geom`, so the same edit touches the same 597 references. It buys crates.io uniqueness and a visible workspace boundary, and it puts a five-letter prefix in every import. Bare names read better; the prefix survives publishing. Decide once, in R1. |
| Why is the envelope `format` a namespaced string at all? | Its only job is to reject a foreign JSON before parsing a document. `"scene"` alone is indistinguishable from another program's `"format": "scene"`; the dot is the usual `owner.thing` convention. The owner need not be a brand: `"furniture.scene"` keeps the check strong and survives any rename, which is why it is the recorded choice. |
| Backwards compatibility with old `.fcs` files? | Not needed — pre-release, no user files exist. Read one `format` string only, not two. |
| Does the settings directory need migration? | Not needed. Renaming it drops the saved theme, recent files and viewport defaults; acceptable before the first release. |
| Is `.fcs` a good extension? | It is `FurniCraft Scene` in disguise, so it should change together with the name. It is **not a constant today**: two production literals in `app/src/file_menu.rs` (the `scene.fcs` default and the `"fcs"` filter) plus the fixtures around them (§5, step 1). |

## 3. Inventory (measured 2026-11-07)

```sh
git grep -o -i furnicraft | wc -l                     # 1042 occurrences
git grep -l -i furnicraft | wc -l                     # 162 files
git grep -o -i furnicraft | cut -d/ -f1 | sed 's/:.*//' | sort | uniq -c | sort -rn
```

| Area | Occurrences | Files | What it is |
|---|---|---|---|
| `crates/` | 488 | 89 | 597 of the 652 Rust hits in the tree are crate references (`furnicraft[-_]<name>`); the other 55 sit on 53 lines |
| `app/` (`apps/furnicraft` today) | 181 | 16 | crate refs + 8 hardcoded `FurniCraft` in `main.rs` + settings paths |
| `docs/` | 65 | 32 | 26 crate refs, 12 `apps/furnicraft` paths, 12 bare `FurniCraft` prose, 14 bare `furnicraft` (the envelope id and `furnicraft_units`-style mentions) |
| `plan/` | 55 | 5 | 19 path hints in `BACKLOG.md`, 18 in `map.md`, 1 in `debt.md` |
| `.opencode/` | 57 | 4 | 32 of them in `skills/enforce-architecture/SKILL.md`; the skills teach the crate map by name |
| `xtask/src/main.rs` | 50 | 1 | the `CRATES` table, `DEV_ONLY`, `TRUCK_ADAPTER`, the `name != "furnicraft"` rule, tests |
| `Cargo.toml` | 15 | 1 | `[workspace.dependencies]` keys |
| `Cargo.lock` | 63 | 1 | regenerated, never hand-edited |
| `state-contract/` | 8 | 5 | doc examples and test fixtures only — cosmetic |
| `vendor/` | 3 | 2 | `vendor/AGENTS.md`, `vendor/truck-shapeops/PATCH.md` |
| `AGENTS.md` / `PROJECT.md` / `README.md` / `ATTRIBUTION.md` / `LICENSE-MIT` / `assets/README.md` | 57 | 6 | rules, crate map, product blurb |

`BRIEF.md` contains no product name: it is the owner's original brief and is never edited to match
the code.

## 4. What the rename costs

Measured by actually doing it in a scratch copy: mechanical rename + build + full test run + clippy +
`cargo xtask layers` green, **1140 tests passing, zero behaviour change**, 960 changed lines in 66
`.rs` files, 81 files total.

| Step | Time | Risk |
|---|---|---|
| 1. One constants home for the product identity (§5 step 1) | 1 h | none — it is a plain refactor with a test |
| 2. Mechanical crate/package rename (§5 step 2) | 2 h including the gate | low; the gate catches every miss |
| 3. Plan path hints and map rows (§5 step 3) | 30 min | the plan gate fails loudly if skipped |
| 4. Envelope `format` + goldens (§5 step 4) | 20 min | 8 fixture lines |
| 5. Prose in AGENTS/PROJECT/README/docs/skills/vendor (§5 step 5) | 1.5 h | drift: four registries must agree |
| **Total** | **~5–6 h, 2–3 commits** | |

After the rename, changing the visible name again is a one-line edit of `PRODUCT_NAME`.

## 5. Procedure

### Step 1 — give the identity one home (do this even if the rename is postponed)

Four constants, one place each, and every call site reads them:

- `crates/ui/src/theme.rs` — `PRODUCT_NAME` (already exists, on-screen words only).
- the app shell — `APP_ID` (machine identity: window class, and the settings directory name). Set it
  through `egui::ViewportBuilder::with_app_id`, because `eframe` otherwise copies the `app_name`
  argument into `viewport.app_id` (`eframe-0.37.0/src/lib.rs:421`) and a marketing rename would then
  silently move `~/.config/<id>` and change the X11 `WM_CLASS` / Wayland app id.
- `crates/project` — `FORMAT` (exists) and a new `EXTENSION`.
- `crates/project` — `GENERATOR` name/version if a `generator` section is ever written; today the
  writer does not emit one.

Then remove the hardcoded duplicates:

- `main.rs`: `.with_title("FurniCraft")` and `"FurniCraft"` as the `run_native` app name → `PRODUCT_NAME`;
  the error string `"FurniCraft requires the wgpu renderer"` → `PRODUCT_NAME`;
  `struct FurniCraftApp` → `App`.
- `app/src/file_menu.rs` — `"scene.fcs"`, `eq_ignore_ascii_case("fcs")` and the dialog listing take
  `project::EXTENSION`.
- `app/src/settings_store.rs` — the three platform branches and `join("furnicraft")` go through `APP_ID`.

Tests: the header already asserts against `theme::PRODUCT_NAME`; add one that the window title, the
`app_id` and the settings directory all come from the constants, and one that the open dialog lists
files by `EXTENSION`.

### Step 2 — mechanical rename

Nothing here needs thought; run it in this order.

```sh
# package names of the 13 library crates
for f in crates/*/Cargo.toml; do
  sed -i -E 's/^name = "furnicraft-(units|geom|kernel|kernel-truck|model|param|mesh|project|scene|render|engine|ui|testkit)"$/name = "\1"/' "$f"
done

# workspace dependency keys that carry a path
sed -i -E 's/^furnicraft-(units|geom|kernel|kernel-truck|model|param|mesh|project|scene|render|engine|ui|testkit) = \{ path =/\1 = { path =/' Cargo.toml

# every member's dependency lines
sed -i -E 's/^furnicraft-(units|geom|kernel|kernel-truck|model|param|mesh|project|scene|render|engine|ui|testkit) = \{ workspace = true \}/\1 = { workspace = true }/' \
  Cargo.toml crates/*/Cargo.toml app/Cargo.toml

# Rust paths and doc references
find crates app -name '*.rs' -print0 | xargs -0 \
  sed -i -E 's/furnicraft_(units|geom|kernel|kernel_truck|model|param|mesh|project|scene|render|engine|ui|testkit)\b/\1/g'

# the app package and its directory
mv apps/furnicraft app && rmdir apps
# then in Cargo.toml: members = ["crates/*", "app", "xtask", "state-contract"],
#                    default-members = ["crates/*", "app"]
# and in app/Cargo.toml: name = "app" (or a neutral binary name, see §8)

rm Cargo.lock          # regenerate; never hand-edit it
cargo fmt --all        # shortening names re-wraps 28 files at max_width 120
```

### Step 3 — registers that must agree in the same commit

1. `xtask/src/main.rs` — every `CrateRule.name`, `DEV_ONLY`, `TRUCK_ADAPTER`, the composition-root
   exception `name != "app"`, the `forbidden` list of `render` (it names `furnicraft-model`), and the
   assertions inside the test module.
2. `AGENTS.md` — the crate map table (§3) and the companion text beside it.
3. `PROJECT.md` — the crate map table (§ around line 212) and the L8 row.
4. `.opencode/skills/enforce-architecture/SKILL.md` — the full per-crate layer list; it is the
   largest single block of names outside the code and it teaches the next agent.
5. `plan/BACKLOG.md` (19 hints), `plan/map.md` (18 paths), `plan/debt.md` (1). The plan gate fails
   on any hint naming a path that no longer exists (`state-contract/src/lib.rs:179`), so these must
   land in the same commit as the move.

`README.md` (`cargo run -p furnicraft`), `PROJECT.md` prose, `assets/README.md` and `ATTRIBUTION.md`
take the same pass.

### Step 4 — the envelope

`crates/project/src/lib.rs:36` is the only runtime string. Change it, then the 8 fixtures under
`crates/project/tests/golden/` carry it on line 2 — rewrite that one line in each, or the loader
rejects them before any migration runs.

Do **not** touch `crates/project/tests/golden/scene-v8.fcs:216` `"generator": {"name": "FurniCraft"}`:
it is the preserved-unknown-section fixture, and `tests/scene_file.rs:112` asserts it survives a read
and a write. It is history, not output.

### Step 5 — prose

`AGENTS.md` §2 says "the product name is always FurniCraft in user-facing text" — that sentence is
the first thing to change. Then `docs/` (32 files: 12 bare mentions in `docs/domain/` and four ADRs,
plus `apps/furnicraft` paths in ten ADRs), `vendor/AGENTS.md`, `vendor/truck-shapeops/PATCH.md`,
`state-contract`'s examples and fixtures.

ADRs are habitually superseded, never rewritten; for a rename a one-line path fix is fine, because
the decision they record did not change.

## 6. Gotchas, each verified

- **`mod mesh;` and `mod project;` live inside `crates/model`.** A crate named `mesh` or `project`
  is shadowed by that module inside the crate, and `mesh::thing()` then resolves to the module, not
  the crate — reproduced as `E0425`. No conflict today (`model` may not depend on either), but if
  that dependency ever appears the module must be renamed (`mod mesh_value;`) or the path written
  `::mesh::…`. Same rule for `crates/ui`'s `mod command;` if a `command` crate is ever added.
- **`ui` is also a parameter name** in 206 `|ui|` closures. `ui::theme::install()` still resolves to
  the crate, because Rust resolves extern crate names in paths before locals — verified in this
  workspace and in a minimal crate.
- **Shortening names re-wraps code.** `rustfmt.toml` sets `max_width = 120`, so `cargo fmt --all`
  rewrites 28 files that were only re-wrapped. Run it before the first gate, not after.
- **Every crate name is taken on crates.io.** Harmless while nothing is published; it is the one
  argument for a prefix.
- **`cargo xtask plan` is the trap.** It reads the hint paths and the commit hashes, and the rename
  commit itself makes the old hashes fine and the old paths dead. Do step 3 in the same commit.
- **`--all-targets` clippy is the real check**, because tests carry most of the render labels and
  fixture paths.

## 7. Gate

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask layers
cargo xtask plan
# or: cargo xtask ci
```

Then check by hand: the window title, the header mark, the Home heading, the settings path, and that
`cargo run -p <app>` still opens the editor.

Measured on the scratch copy: 1140 tests, 0 failures, `layers: ok`, clippy clean.

## 8. Open questions for the owner

- The final product name. Nothing here hardcodes it, so it can land after R1.
- The app package and binary name: `app` (neutral, `target/debug/app`) or `editor`
  (`cargo run -p editor`). The directory and the package can differ — a `[[bin]] name` overrides the
  binary only.
- The envelope identity once the brand is gone: `"furniture.scene"` keeps a namespace and survives
  any name; `"scene"` is the cheapest and the least defensive. Same question for the extension:
  `.fcs` should be replaced, and what with.
- Whether `APP_ID` should be a reverse-DNS id (`dev.<owner>.<product>`) or a short word; the former is
  the convention for `.desktop`, `flatpak` and Wayland, the latter is what the settings path shows.

## 9. Re-orienting in five minutes

Do not re-read the tree. These commands answer everything this file answers, and they are cheap.
Paths below are the **current** ones (`apps/furnicraft`, `crates/furnicraft-*`); after step 2 the
same file is under `app/` and the crates are bare, so translate the path and run them again.

```sh
# how much is left, and where
git grep -o -i furnicraft | cut -d/ -f1 | sed 's/:.*//' | sort | uniq -c | sort -rn
git grep -l -i furnicraft | wc -l

# is a hit a crate reference (mechanical) or a word (a decision)?
git grep -n -E 'furnicraft[-_][A-Za-z]' -- '*.rs' | wc -l     # mechanical: 597
git grep -n -E 'furnicraft([^-_A-Za-z]|$)' -- '*.rs'          # needs a decision: 11 lines

# the four places that are identity, not names
git grep -n 'PRODUCT_NAME' -- '*.rs'
sed -n 36,40p crates/project/src/lib.rs                      # FORMAT
sed -n 100,120p apps/furnicraft/src/settings_store.rs        # the three platform branches
grep -n 'with_title\|run_native' apps/furnicraft/src/main.rs

# the registers that must agree (a miss here fails the gate or leaves the docs lying)
sed -n '/const CRATES/,/^];/p' xtask/src/main.rs
grep -n 'apps/furnicraft' plan/BACKLOG.md plan/map.md plan/debt.md   # 38 path hints
grep -c -i furnicraft .opencode/skills/enforce-architecture/SKILL.md
```

The per-file list after the mechanical pass is short enough to keep in the head: `crates/render`
(20 debug labels), `apps/furnicraft/src/main.rs` (8), `apps/furnicraft/src/settings_store.rs` (5),
`xtask` (4), `crates/project/tests/scene_file.rs` (4), then one to three hits in ~25 files, mostly
`//!` lines.

To see what the finished rename looks like, re-create it from §5 step 2 — the whole mechanical part
is four `sed`s and one `mv`, and it builds in one `cargo build --workspace`.
