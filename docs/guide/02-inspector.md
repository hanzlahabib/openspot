# 2. Inspector

The Inspector scans continuously and lists every network in range. It does not need a project or
a floor plan. It is the right tool for "what is around me and how strong is it", for picking a
channel, and for checking a single spot.

## The controls bar

From left to right:

- **Adapter** picker. Changing it clears the table and starts scanning with the new adapter.
- **Pause / Resume** and **Scan now**. A paused Inspector stays paused until you resume it.
- **Scan interval**: every 1, 2, 5, 10 or 30 seconds (also in Preferences). Shorter intervals
  show changes sooner but keep the radio busier.
- **2.4 GHz / 5 GHz / 6 GHz** checkboxes and a **text filter** that matches SSID, BSSID and vendor.
- **Group** picker, **Groups...**, **Export CSV**, **Columns**, **Aliases** (see below).

The status bar shows the number of networks and scans so far, "paused" when paused, and the
latest notice or error.

## The table

One row per BSSID (one radio of one access point). Click a column header to sort, click again to
reverse. Click a row to select it; selected rows are drawn in the charts. Right-click a row for
**Set alias...**, **Remove alias** and **Group**.

Columns (all can be shown or hidden under **Columns**; **Reset columns** restores the defaults):

| Column | Meaning |
|---|---|
| SSID | Network name. Hidden networks have an empty name and show their BSSID. When an alias exists it replaces the SSID here. |
| Alias | Your alias only. Off by default. |
| BSSID | Radio MAC address. |
| dBm | Signal now. |
| % | The same signal as a percentage: linear from 0 % at -100 dBm to 100 % at -50 dBm, the convention Windows documents for its own signal-quality number. |
| Avg, Min, Max | Average, weakest and strongest signal seen since the network appeared. |
| Ch | Primary channel. |
| Band | 2.4, 5 or 6 GHz. |
| Width | Channel width read from the beacon's information elements. |
| PHY | Generation(s) the AP advertises (a/b/g, n, ac, ax, be). |
| Security | Open, WPA2, WPA3 and so on, read from the beacon. |
| Type | Infrastructure or ad-hoc. Off by default. |
| First seen | Time since first sighting. Off by default. |
| Seen | Time since last sighting. |
| Vendor | Manufacturer from the first three bytes of the BSSID (see below). Locally administered ("private") addresses, common on phones and some mesh systems, show "(private address)". |

Your choice of columns, grouping, filter checkboxes and sort persists between runs (sort order and
the text filter do not).

**Vendor lookup** uses the public IEEE registry, bundled with the program, restricted to the
24-bit (MA-L) assignments. Devices in the longer MA-M and MA-S blocks therefore show no vendor.
This is a known gap ([Limits](11-limits.md)).

**Stale networks.** A network that has not been heard for a while is removed from the table. The
default is one minute; choose 5 minutes, 15 minutes or never in Preferences, Inspector.

## Grouping

**Group** offers: No grouping, SSID, Channel, Band, Security, Vendor and Custom group. Each group
has a header row (click it to collapse); rows without a value for the chosen attribute go to an
**Ungrouped** bucket at the end.

**Custom groups** are yours: open **Groups...** to create, rename and delete them, then right-click
rows and use **Group** to add them (or **Remove from group**). When several rows are selected,
the action applies to all of them. Custom groups are stored in the preferences file, so they apply
to the whole application.

## Aliases

An alias is a name you give a BSSID, such as "Hallway AP". Right-click a row and choose
**Set alias...**. Aliases show in the table, the charts and in Survey, and appear in reports.

- They are **application-wide**, not per project. A project does not carry its aliases; the
  roadmap lists per-project aliases as not done.
- **Aliases** menu (and Preferences, Aliases): import from CSV, export to CSV, forget all. The
  file has two columns, `BSSID` and `alias`. Import can be *strict* (any bad line rejects the whole
  file) or lenient (good lines are applied, bad ones listed). A preview shows what will change, and
  you choose whether imported names overwrite existing ones or are skipped.

## Charts

The lower panel has tabs:

- **Signal over time**: lines for the selected rows, with a window of the last 1, 5, 15 or 60
  minutes. Click rows in the table to add them. Nothing is drawn until at least one row is selected.
- **2.4 GHz channels, 5 GHz channels, 6 GHz channels**: each network drawn at its real position and
  width on the spectrum, so overlap is visible. Selected networks are highlighted.
- **Channel advisor**: see below.
- **Event log**: see below.

## Channel advisor

Pick a channel width (20, 40, 80 or 160 MHz). For each band the advisor lists up to four
channels, least congested first. For every candidate channel it adds up the power of all networks
currently heard whose spectrum overlaps the candidate, in proportion to how much of the candidate they
cover, and converts the sum to dBm. The label comes from that level:

| Label | Combined overlapping power |
|---|---|
| clear | at or below -82 dBm (the level at which radios typically do not defer to a signal) |
| moderate | up to -72 dBm |
| busy | above -72 dBm |

Each line also shows how many networks overlap and marks DFS channels in 5 GHz. On 2.4 GHz only
channels 1, 6 and 11 are offered at 20 MHz. The recommendation is based on what the adapter hears
**from where you are standing right now**. Move around and check again before changing a router's
channel. It does not know about non-Wi-Fi interference such as microwave ovens or Bluetooth, and it
cannot see the traffic load of a network, only its signal.

## Event log

The Events tab records what happened: scans (how many networks, which appeared or disappeared),
pause and resume, adapter changes, interval changes and errors (repeated errors are folded into one
entry with a count). Filter by category or text, **Export text...** or **Export CSV...**,
**Clear**, or set "Erase older than" (never, 15 minutes, 1 hour or 24 hours). The log is capped at
5000 events whatever the setting.

## CSV export

**Export CSV** writes the table as currently filtered, sorted and with the visible columns, using a
comma, semicolon or tab delimiter (semicolon suits spreadsheets in countries that write decimals with
a comma). Cells that a spreadsheet could treat as a formula (starting with `=`, `+`, `-` where not a
plain number, or `@`) get a leading apostrophe, so opening the file cannot run anything.

## Sound

Preferences, Inspector can play a sound when a network appears that was not in the previous scan.
It is off by default and implemented on **Windows only**; on other systems the setting does nothing.
