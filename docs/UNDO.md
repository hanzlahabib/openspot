# Survey undo/redo (V-13)

Ctrl+Z / Ctrl+Y (Cmd on macOS) walk a last-in-first-out history of survey edits.

| Undoable | Not undoable (clears the history) |
|---|---|
| Measurement point added (with its speed results) | Loading or drawing a floor plan, PDF import, blank plan |
| Floor-plan scale set or calibrated | Deleting a zone |
| Snapshot added, deleted or merged | Opening or creating a project |
| Zone rename, snapshot rename | |

## Why map loads and zone deletion are excluded

Edits are stored as small operations, not whole-project snapshots, because a floor plan is a
multi-megabyte image. Making a plan load undoable would mean keeping the previous image (and,
for a zone, everything in it) for every step, which breaks the memory bound. Both operations
also shift or replace what older edits point at, so the history is dropped rather than left
pointing at the wrong place.

## Memory bound

History keeps at most 500 edits and at most `MAX_COST` (50 000) records, where a record is a
scan point, an access-point reading or one small edit. An access-point record with its raw
information elements is about half a kilobyte, so the worst case is roughly 25 MB. The oldest
edits are dropped first. Deleting a snapshot stores the whole snapshot; one that alone exceeds
the budget is deleted without an undo entry, the history is dropped and the status line says
so.

## Consistency rule

Edits refer to zones and snapshots by index. This is safe because every change to those lists
is either recorded (undo restores the old layout before older edits run) or clears the
history. A new edit after an undo discards the redo stack.
