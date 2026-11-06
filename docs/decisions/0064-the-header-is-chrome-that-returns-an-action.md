# 0064. The header is chrome and returns an action

- Status: Accepted
- Date: 2026-11-06

## Context
The File menu drew its own `Panel::top`, so it owned a row by itself: the product name, the file
state and the only menu in the application shared a strip nothing else could use. The viewport then
repeated the product name in its first row, and the row also carried the undo/redo entries, the view
controls and the status text. The owner asked for a header where identity, file actions, history and
open files sit in one line, with dividers between the meaning groups.

Two decisions had to be taken. The window keeps its system frame (the owner's choice: a custom
title bar costs resize-from-edge handling on every platform), so the header draws no close,
minimize or maximize button. And the header must not become a second way into the document.

## Decision
`apps/furnicraft/src/header.rs` draws one row and returns at most one `HeaderAction`
(`Home`, `Undo`, `Redo`, `Save`, `Settings`, `CloseTab`). The application performs it. Nothing in the
header touches `Project`, dispatches a command or reads the filesystem:

- `Save` goes through `FileMenu::save`, which is the same `MenuCommand::Save` the File menu uses, so
  the two cannot drift into one saving silently and the other opening a path dialog;
- `Undo` and `Redo` call `Session::undo`/`redo` and are disabled when `can_undo`/`can_redo` is false;
- `Home`, `Settings` and `CloseTab` are reported to the shell, which owns the surfaces they will open
  (U41 and U45); until those land, `Home` and `Settings` report their absence in the status bar rather
  than pretending;
- the File menu keeps its dialogs, path handling and confirmation; the header only gives it a place.

The row is laid out with `egui::Sides::shrink_right`, so the gear is pinned to the far edge however
wide the tab strip grows — two nested layouts left it floating where the tabs happened to end.

The system frame stays, so the `window` icon theme exists for a future frameless mode but no header
button draws it today.

## Consequences
The header can grow a tab per open file (U45) without changing its contract: it reads a `HeaderState`
and returns an action, and the shell decides what a tab means. A themed `theme::divider` separates the
groups, so the header and the ribbon share one rhythm.

Keeping the system frame costs the design one line of the brief — the window buttons — and buys
reliable resizing, snapping, and window-manager tiling on Linux. The `window` theme is one file set
away when a frameless mode is wanted, and its footprint/coverage tests already run.
