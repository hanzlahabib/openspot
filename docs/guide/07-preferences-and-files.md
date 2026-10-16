# 7. Preferences and files

## Preferences window

Open it with **Preferences** in the bar at the top. Changes apply immediately and are saved a moment
after you stop changing them and when you quit.

| Page | What you can change |
|---|---|
| Inspector | Scan interval, signal chart window, how long before an unheard network is removed, how long event-log entries are kept, new-network sound |
| Survey | Length unit; show heatmap, points and AP markers on the plan |
| Heatmap | Colour gradient, smoothing and opacity |
| Aliases | Import, export and forget aliases |
| Appearance | Theme (follow system, light, dark), high contrast, interface size (100, 115, 130, 150 %), **Language** |
| Backup | Nothing yet: automatic project backups are not available. Saving already keeps a `.bak` copy of the previous version |
| Privacy | A statement of what OpenSpot does and does not send |
| Updates | The update check (below) |

**Language.** Only English is included. Choosing "Follow system" or "English (United States)" is all there is to choose, and
the mode bar and this window are the only parts of the interface that go through the translation system
so far. See [I18N.md](../I18N.md) for how translations are added.

## Privacy

OpenSpot has no telemetry, no analytics and no accounts. Scan results, projects and aliases stay
on your computer. The **only** network requests it can make are:

- the **update check**, which is **off by default**. When on (Preferences > Updates, or **Check now**) it makes one
  request to GitHub's public releases page and sends nothing about you or your networks;
- the **active scan** tests you configure, to servers you name (see [Active scan](05-active-scan.md)).

(The repository URL used by the update check is still a placeholder in this build, so the check
cannot succeed until a real repository is configured.)

## Files

| File | What it is |
|---|---|
| `name.ospot` | A project: one SQLite database holding zones, snapshots, measurements, plans and settings |
| `name.ospot.bak` | The previous version, copied just before each save over an existing file. One generation only; the next save replaces it |
| `name.ospot.autosave` | Written periodically while a project has unsaved changes (default every 2 minutes), never over the main file. Removed after a normal save. If one is newer than the project when you open it, OpenSpot offers to recover it |
| `name.ospotz` | A project archive for sharing: a zip with `manifest.json` and `project.ospot` (see [Reports and archives](06-reports.md#project-archives-ospotz)) |
| `prefs.json` | Your preferences, aliases, custom groups and report branding |

Saves write a temporary file and rename it into place, so a crash while saving cannot destroy the previous
file. You can copy a `.ospot` file like any other file; it is a standard SQLite database.

### Where `prefs.json` lives

In the operating system's configuration folder, in an `OpenSpot` subfolder:

| System | Location |
|---|---|
| Windows | `%APPDATA%\OpenSpot\prefs.json` (usually `C:\Users\<you>\AppData\Roaming\OpenSpot\prefs.json`) |
| Linux | `~/.config/OpenSpot/prefs.json` (or `$XDG_CONFIG_HOME/OpenSpot/prefs.json`) |
| macOS | `~/Library/Application Support/OpenSpot/prefs.json` |

It is plain JSON. If it cannot be read or is corrupt, OpenSpot starts with defaults and shows a message in
the bar at the top; it does not overwrite your file until something changes. Delete it to reset everything.
Unknown entries from a newer version are ignored, so downgrading will not break startup.

### Aliases are not in the project

Aliases live in `prefs.json`, not in `.ospot` files. A project you send to someone shows BSSIDs and SSIDs, not your
aliases; send an alias CSV (Inspector > Aliases > Export) along with it if needed. Per-project aliases are not implemented.

## Updating

Check the releases page of the repository, or turn on the update check. OpenSpot does not download or install
anything by itself.
