# 0065. Visibility and lock are session state

- Status: Accepted
- Date: 2026-11-06

## Context
`plan/questions.md` carried an open question: whether a node's *visible* and *locked* flags are
document data or session state. The scene tree the owner asked for draws an eye and a padlock per
row, so the answer now blocks the icon set and the tree layout, and the wrong answer would be
expensive later — a flag in the `.fcs` means every visibility toggle is an undoable document change,
a version bump and a round-trip through every command that copies a node.

The constitution already separates the two kinds of state: `AGENTS.md` §4.10 puts the node tree,
placements and authored cameras in the document, and puts selection, hover, open containers,
viewport navigation, projection and grid in the session or the shell. Visibility and lock are the
same kind of thing as selection: they say what the current user is looking at and what the current
user may touch, not what the furniture is.

## Decision
Visibility and lock are **session state**, keyed by node `Id`, held next to the selection in the
editor and the shell. The owner confirmed this: the flags belong to the current session and are not
written to the document.

- `Editor` (or a small `ViewFlags` beside the selection) owns `hidden: HashSet<Id>` and
  `locked: HashSet<Id>`; both are dropped with the session, not saved, and not part of any command.
- The scene builder skips a hidden subtree when it composes a frame, and picking skips it too, so a
  hidden node cannot be selected by a viewport click — the two consumers agree because both read the
  same set.
- A locked node is still drawn and selectable; a gizmo drag and a destructive command refuse it,
  and the status bar says why. Locking is a guard on a gesture, not a document rule, so it is checked
  in the shell before a command is dispatched rather than inside the command.
- The flags are keyed by `Id` and pruned like the open-container set: a node that vanishes takes its
  flags with it, so an undone deletion never resurrects a stale hidden state.
- The icon set gains `actions/visible`, `actions/hidden`, `actions/lock`, `actions/unlock` use in the
  tree; they exist as placeholders today and are drawn by the icon task.

## Consequences
Toggling visibility costs no revision, leaves no undo entry and never appears in a diff between two
scene files: the same document opens fully visible on another machine, which is what a colleague
expects. The price is that a hidden or locked state is lost when the session ends — the same trade
the open-container and selection state already make, and the owner accepted it for them.

If a future workflow needs a node hidden *in the saved file* (a construction reference, a jig body),
that is a different concept — an object property with its own name — and it gets its own decision
rather than being folded into this one.
