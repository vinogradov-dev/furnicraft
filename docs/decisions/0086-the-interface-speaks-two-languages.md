# 0086. The interface speaks two languages, and a generated name keeps the one that made it

- Status: Accepted
- Date: 2026-11-11

## Context

Every caption, hint, menu entry and refusal in `crates/ui` and `apps/furnicraft` was an inline
English literal: about 730 of them. The owner wants the interface to speak Russian as well, and a
language chosen in the workspace settings, not in a scene file. A second language is also the test
of the surface: a string that is built by concatenation or hidden in a `thiserror` message cannot be
translated without changing the code that draws it.

Two shapes were on the table: a gettext-like catalogue keyed by the English sentence, and a typed
enumeration of messages.

## Decision

- **One typed table.** `crates/text` (L0, no dependencies) holds `Lang::{En, Ru}` and one `Message`
  variant per user-facing string. `Message::table` returns `&'static [&'static str; Lang::COUNT]`
  and `Message::text(lang)` indexes it. The variant is the key; the translation sits beside it.
- **A missing translation is a compile error.** `Lang::COUNT` is the length of the table, so adding
  a language variant turns every row into a type error until the new column is written. There is no
  English fallback to hide a gap.
- **Arguments are placeholders, not concatenation.** A sentence with a value carries `{}` and
  `Message::with(lang, args)` splices the values, so a translation may put them in another order.
  A counted sentence is a `Counted` built by `Message::counted(n, one, few, many)`, which picks
  Russian's one/few/many form itself; English uses one and many.
- **The language is workspace settings.** `Settings::appearance.language` is a `Language` enum in
  `settings.rs`, chosen in the *Settings* window and persisted with the rest of the workspace. The
  shell publishes it on the `egui` context once per frame, so a surface reads it without threading a
  `Lang` through every signature; the `.fcs` never carries it.
- **A generated name is fixed in the language that created it.** `{} drawing`, `{} body`, a new
  panel, assembly, camera, light or group name is written into the document when it is created and
  never re-derived. Switching the language afterwards renames nothing.
- **What is not translated.** Command ids, `FurnType` numbers, material and node names, `mm` and the
  `thiserror` messages of `model`, `units` and `project`: the first are identity, the last are log
  and test text. A refusal that reaches the screen is mapped to a `Message` at the UI boundary
  (`length_error`, `degrees_error`, `diagnostic_message`), never printed raw.
- **A command label is a `Message`.** `CommandSpec::label` returns the key, so the ribbon, the
  palette and a future CLI print the same words; the `id` stays the identity.

## Consequences

The interface reads as one language at a time, and a missing translation cannot ship. The cost is
that every drawing site holds a variant name instead of a sentence, and a translator works against
the variant keys in `crates/text/src/messages.rs`; a future move to per-language files reuses the
variant as the key and changes only where the table is stored. The ribbon cell floor moved from 104
to 108 pt because the longest Russian caption measures 98 pt (ADR 0067, amended there).

Revisited if a third language needs a grammatical form the one/few/many model cannot express, or if
a translator needs to work outside the repository: the fix is the per-language file, not a second
lookup path.
