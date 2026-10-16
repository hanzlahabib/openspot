# 8. Keyboard and mouse

OpenSpot has few shortcuts. This list is read from the code; anything not listed here does not exist.
"Ctrl" means Cmd on macOS.

## Keyboard

| Keys | Where | Does |
|---|---|---|
| Ctrl+Z | Survey | Undo (measurement points, scale, merge) |
| Ctrl+Y or Ctrl+Shift+Z | Survey | Redo |
| Enter | Draw tool | Finish the line being drawn |
| Delete | Draw tool | Delete the selected shape |
| Esc | Draw tool | Abandon the current drawing action |
| Esc | Preferences window, alias dialogs, group dialogs, alias import preview | Close it |
| Enter | Alias, group and rename fields | Confirm |
| Tab, Shift+Tab | Everywhere | Move between controls |
| Space or Enter | Everywhere | Activate the focused control |

Undo and redo are ignored while you type in a text field. The Survey dialogs (scale, blank plan, merge and so on) are closed with their Cancel buttons; they do not respond to Esc.

## Mouse (Survey plan)

| Action | Does |
|---|---|
| Click | Does what the active tool does: measure, add a waypoint, add a crop corner, draw, or pick a calibration point |
| Drag (left or middle button) | Pan the plan |
| Scroll wheel or pinch | Zoom around the pointer |
| Hover | Details of the point under the pointer, or the heatmap value there |
| Drag a diamond | Place an access point marker by hand |
| Drag a shape (Draw tool, Select) | Move it |

The **Zoom** menu offers Fit to window, 25 %, 50 %, 100 %, 150 % and 200 %.

## Mouse (Inspector)

| Action | Does |
|---|---|
| Click a row | Select or deselect it for the charts |
| Click a column header | Sort; click again to reverse |
| Click a group header | Collapse or expand it |
| Right-click a row | Alias and group menu |

## Accessibility

Tab order follows the screen, and **Preferences > Appearance** offers a high-contrast theme (black and
white with strong outlines, text at the WCAG AAA 7:1 ratio) and larger interface sizes. Screen-reader support depends
on the toolkit (egui with AccessKit) and has not been tested with a screen reader.
