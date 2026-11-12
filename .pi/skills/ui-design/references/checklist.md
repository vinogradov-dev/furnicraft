# Review form

Answer every line **PASS**, **FAIL** or **n/a** with one line of evidence (file and symbol, a
measured number, or the test name). A line without evidence is not answered. The form is a checklist,
not a score: one FAIL blocks the change, and a FAIL you decide to accept goes to `plan/debt.md` in
the same change.

## A. Function

1. One primary action on the surface; a second one is a menu, a palette or a separate screen.
2. The person can tell what the surface acts on: the selected node, the armed tool, the active view.
3. Every control changes something or says why it cannot (disabled with a reason, not a tooltip).
4. The action is reversible by undo, or it asks once and names what it destroys.
5. No document mutation outside a command; no revision written by a fold, a hover or a camera move.

## B. Perception

6. Feedback starts on the press frame; a rebuild over the budget announces itself first.
7. Nothing is distinguished by colour alone; selection, error and focus each have a second cue.
8. Contrast measured with `theme::contrast_ratio` meets 4.5:1 (text) / 3:1 (icons, boundaries) in
   **both** variants of every preset the change touches.
9. A floating surface stays legible over the model at `MIN_PANEL_OPACITY` and at 100 %.

## C. Motor

10. Every target is at least 24 × 24 pt, and a new target has slack around its drawn shape
    comparable to the existing gizmo hit radii.
11. Click and drag are separated by a distance threshold; a drag captures the pointer and survives
    leaving the widget.
12. Nothing is reachable only by hover, only by double-click, or only by an undocumented modifier.
13. Esc, a second click and undo restore the previous state of every armed tool and preview.

## D. Consistency

14. Colours, radii and heights come from `theme.rs`; no literal colour outside it.
15. The action already existed elsewhere: same icon, label, shortcut and place, or the old one is
    removed in this change.
16. State that is not document data lives in the session or the shell, not in `Project`.

## E. Text and language

17. Every string is a message id in `crates/text`; no literal in a widget.
18. The unit is outside the field; the term is the glossary's term; the label is the verb.
19. Both languages read correctly and neither is cut off by the layout.

## F. Scope

20. A new default, mode, gesture or second way to do one thing has the owner's approval and an ADR.
21. The change updates `references/surfaces.md` when it adds a surface, a gesture or a shortcut.

## Eyes only

These cannot be decided from code, a rectangle or a test. Name the check that was used:

- translucency of a floating panel over the model, at its two opacity ends;
- the light theme (a colour that passes on graphite can disappear on paper);
- 55 %, 92 % and 100 % panel opacity with both panels folded and hidden;
- 100 % and 200 % UI scale, and a long node name in a narrow panel;
- the feel of a drag: does the object follow the pointer, does it overshoot, is the snap visible;
- the icon at the size it is drawn: is the action clear at 16 px without its label.

## How to answer a review

```text
verdict: PASS | FAIL (n items)
A1 PASS  crates/ui/src/…: the card's one primary action is Apply
C10 FAIL …: the caret face is 8 x 8 pt, under 24
C10 FAIL accepted → plan/debt.md "[ui] the caret face is 8 pt"
eyes: light theme not checked — needs an offscreen render or a live run
```

Do not rewrite the code during a review unless asked; a review that edits and judges at once hides
its own mistakes. Do not invent rules: a verdict cites this form, an ADR, or a measured value.
