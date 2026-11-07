# 0068. The workspace lives outside the document

- Status: Accepted
- Date: 2026-11-07

## Context
U41 adds a Settings window (theme, accent, UI scale, viewport defaults, ribbon opacity, shortcuts)
and a Home start surface (new, open, recent files, the empty template). All of that is *how the
program looks and starts*, not part of the cabinet. The document already has a versioned, atomic
envelope in `furnicraft-project`; the UI scale, an accent colour or a recent-file list inside a
`.fcs` would travel to another machine with the furniture, make two users of the same file fight
over their window, and put presentation into every undo/revision path.

## Decision
The workspace is one JSON file at the operating system's config directory
(`%APPDATA%\FurniCraft`, `~/Library/Application Support/FurniCraft`,
`$XDG_CONFIG_HOME/furnicraft` or `~/.config/furnicraft`), named `settings.json`.

- The data model (`Settings`, `Appearance`, `ViewportSettings`, `RibbonSettings`, `ThemeChoice`,
  the display enums) lives in `furnicraft-ui`; it is versioned (`SETTINGS_VERSION`) and every ranged
  value is clamped on read.
- Persistence lives in the shell (`apps/furnicraft/src/settings_store.rs`), because the UI crate does
  no file I/O. The write follows the scene file's shape: a temporary sibling is flushed, synced and
  renamed over the target, so a crash cannot leave a half-written file.
- A missing, unreadable or foreign-version file is not fatal: the loader returns the defaults and a
  one-line reason, which the shell shows in the status bar. The Home surface and the recent list come
  from the same file; a path enters the list only when a scene was actually opened or saved.
- The Settings window edits the running application at once (theme, accent, scale, grid, ribbon
  opacity), but the file is written when the window closes and on exit; the document revision and
  undo history never change.
- The scene file carries none of it. The existing test that serialises a `Project` and looks for the
  settings keys keeps that boundary honest.

## Consequences
A scene is portable; a workspace is personal. Settings can be reset by deleting one file, and a
corrupt settings file costs the user their preferences rather than their project. The cost is a
second store with its own path rules, version and migration: the next settings field has to be added
to the schema, the clamping and the round-trip test, and an old file deliberately falls back to the
defaults instead of being migrated in place.
