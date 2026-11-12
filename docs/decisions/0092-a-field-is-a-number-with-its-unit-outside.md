# 0092. A numeric field is a number with its unit outside, and one edit is one undo entry

- Status: Accepted
- Date: 2026-11-12

## Context

The inspector's numbers were ad-hoc: `placement_card` and `pivot_card` drew a 96 pt text edit whose
buffer held the formatted value *with its unit* (`"12.5 mm"`, `"30°"`), so selecting the number selected
`mm` too and a pasted number had to survive a unit. Six fields shared one Apply button, every change
cost a click, and typing «120» rebuilt the panel three times because the text committed whenever the
field changed. The scrub widget that already existed (`scrub.rs`) clamped and rounded correctly but
drew a button-sized box that grew with the number, showed the unit inside the box, and committed only
on blur. The owner asked for numbers only at 76 px, immediate feedback with a short debounce, one undo
entry per value, and no per-field Apply.

Undo made the last part the hard one: `Session::dispatch` pushes the document before every command, so a
field that writes while the hand moves would leave one history entry per frame.

## Decision

**Field anatomy.** `scrub.rs` draws one box: 76 x 22, 4 pt radius, right-aligned in the theme's
monospace face (its digits are the tabular figures), and the unit as a dim caption allocated *after*
the box. The editable string is a bare number; the parsers accept an optional unit anyway, so pasting
`18.5 mm` still works. The space is reserved with `Ui::allocate_space` and the box is one interaction
under one id — an allocation with `Sense::hover` under that id tells egui the widget cannot take focus
and egui drops the focus it just gave it.

**Commit policy.** `CommitPolicy::Live` writes the document on every frame of a drag; `CommitPolicy::
OnRelease` shows the running number and writes it once at the release, for a value whose rebuild is
expensive (a chamfer, a bow, a cutout). Typed text always waits 150 ms after the last keystroke, or
Enter, or blur: committing per keystroke is the rebuild the owner objected to, so there is no policy
that does it. Invalid text keeps the value, marks the box red and reports the reason for the card's
error line.

**One undo entry per edit.** `Session::edit_scope` mints a token and `Session::dispatch_in(scope, …)`
folds consecutive dispatches that carry it into the entry pushed by the first one. `Session::cancel_edit`
restores the snapshot the edit began with, drops the entry and clears redo, so Escape during a drag
leaves no trace. A field keeps its token in egui memory and reports it in `FieldOutcome::scope`; the
card mints one token per frame with `Session::edit_scope` before it borrows the document. Undo, redo and
any plain dispatch close the open scope, so an unrelated command is never folded into a field's edit.

## Consequences

- A drag, a typed number and a preset all reach the same command; the card converts to `Length` or
  `Degrees` once, at the command edge, and the document's exactness is preserved by touching only the
  component the field owns — a translation edit keeps the stored quaternion bit for bit.
- The command bar for a previewing operation can reuse the check/cross pair and the same field, but it
  needs a preview that is not a document revision first (M71); no per-field Apply survives it.
- The object card's figure and primitive parameters and a typed property are still text edits (M72):
  they are the same conversion, not a second design.
- Revisit if a field ever needs to show a unit inside the box for width reasons; the suffix outside is
  the rule, and a header that names the unit («Размеры, мм») is the alternative the field supports by
  leaving `unit()` empty.
