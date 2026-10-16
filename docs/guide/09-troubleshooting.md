# 9. Troubleshooting and FAQ

Run the command-line scanner first. If it works there but not in the GUI, the problem is in the GUI;
if it fails there too, the problem is the operating system, the adapter or permissions.

```sh
openspot adapters
openspot scan
```

## No networks are listed

Work down this list.

1. **Wi-Fi is off.** Quick Settings, airplane mode, or a hardware switch. OpenSpot reports "the Wi-Fi
   radio is switched off".
2. **Windows 11 24H2 and later: location access is off.** The system returns an empty or
   connected-network-only list without any error. Enable Settings > Privacy & security > Location >
   **Location services** and **Let desktop apps access your location**. See
   [Getting started](01-getting-started.md#windows-location-access).
3. **The WLAN service is not running** ("WLAN AutoConfig"). Run `net start wlansvc` from an elevated
   prompt, or start it in `services.msc`.
4. **No adapter.** "No wireless adapters found". Plug in or enable one. A desktop without Wi-Fi hardware,
   a virtual machine and WSL have none.
5. **Wrong adapter.** If you have several, choose the right one in the Inspector's adapter picker.
6. **Filters hide everything.** Check the band checkboxes and the text filter. The table then says "No networks match the
   filters".
7. **It is just early.** The first scan takes a few seconds; the table says "Scanning..." until it arrives.

## Permission and access errors

| Message | Meaning and fix |
|---|---|
| access denied; on Windows 11 24H2+ enable Location services for desktop apps | Location access is off for desktop apps; see above |
| the WLAN service is not running | Start "WLAN AutoConfig" (`net start wlansvc`, elevated) |
| insufficient privileges to control the Wi-Fi adapter | Linux: triggering a scan needs the `CAP_NET_ADMIN` capability. Run as root or grant it: `sudo setcap cap_net_admin+ep $(which openspot)` (use the GUI binary's path for the GUI). **Unverified on hardware** |
| the Wi-Fi radio is switched off | Turn Wi-Fi on |
| scan timed out waiting for the driver | The driver did not answer in time. Try again; if it repeats, update or reinstall the adapter driver |
| no native Wi-Fi backend for this operating system yet | This build has no scanner for your system |

## The list is shorter than my phone's

The adapter reports only what its driver returns, and some drivers return a cached list that refreshes slowly.
OpenSpot merges several probes (`openspot scan --probes 3`) and the GUI keeps scanning, so networks accumulate.
An adapter that does not support 5 GHz or 6 GHz will never list networks on those bands.

## 6 GHz, Wi-Fi 6E and Wi-Fi 7

- Networks in the 6 GHz band appear only if **your adapter and driver support 6 GHz** (Wi-Fi 6E or later)
  and **your region allows it**. A Wi-Fi 6 (or older) adapter lists 2.4 and 5 GHz only. This is a hardware limit, not a setting.
- OpenSpot reads the PHY generation and width from the beacon. The Wi-Fi 7 (802.11be) capability and operation elements are parsed,
  including 320 MHz widths, so the **PHY** and **Width** columns and the PHY coverage map handle them; this is covered by unit tests, not by
  a real Wi-Fi 7 network. Multi-link operation is not interpreted.
- The 6 GHz channel chart and advisor exist; the 6 GHz path has been exercised with built-in sample data
  and unit tests, **not** against a real 6 GHz network. Treat it as unverified.
- In the 5 GHz advisor, channels in the DFS range are marked; a router that moves off a DFS channel when radar is detected
  can make DFS channels less stable in practice, which the advisor cannot know.

## Why are there no SNR or noise maps?

SNR needs the noise floor, and **Windows does not give applications a noise value.** The Native Wifi API reports signal
strength and link quality per network, but no noise, and no vendor extension on the tested adapter did either. Rather than
invent a number (a fixed "-95 dBm" would turn SNR into signal strength under another name), OpenSpot shows no SNR or noise map.
The research and what could work on Linux and macOS is in [NOISE_FLOOR.md](../NOISE_FLOOR.md).

What to use instead: the **signal-to-interference** map ([Heatmaps explained](04-heatmaps.md)) captures co-channel
competition from other Wi-Fi networks, which is the dominant problem in most buildings. It cannot see non-Wi-Fi noise.

## The heatmap looks wrong

- **Smooth where I expected a wall.** The map interpolates linearly between points and does not know where walls are.
  Measure on both sides of walls.
- **Colours stop in the middle of nowhere.** The map reaches only as far as the *data point radius*. Choose a larger preset in
  **Sampling**, or measure there.
- **Everything is one colour.** The range may be too wide or narrow for your data; set the range under the legend, or press
  **Reset range**. Troubleshooting thresholds may also be set far from your values.
- **Distances or radius presets look off.** Check the scale (status bar says "scale not set" in yellow when missing).
- **I see only part of the map.** A **crop** may be active (Crop tool > Remove crop).
- **A network is missing from the map.** It may be unticked in the list, hidden-network inclusion may be off, or it was heard at too few points.

## Measuring problems

- **Clicking does nothing.** The Measure tool must be active, a plan must be loaded, and a scan must not already be running.
  A calibration in progress takes the clicks.
- **Points in the wrong place after a path run.** Path mode assumes you walked at the set speed along the route.
- **No access point diamonds.** An AP needs at least three readings and must be ticked in the list.

## Files

- **"preferences file is corrupt".** OpenSpot started with defaults. Fix or delete `prefs.json` (see
  [Preferences and files](07-preferences-and-files.md#where-prefsjson-lives)).
- **I closed without saving.** Reopen the project and look for the recovery offer. Otherwise the `.ospot.autosave` file next to
  the project holds the last autosave, and `.ospot.bak` the version before your last save.
- **I overwrote a project by mistake.** `name.ospot.bak` holds the previous version if you saved at least once more after opening.

## Reporting a problem

Open a GitHub issue with the version, operating system, adapter, and what happened. If a scan looks wrong, attach
`openspot scan --json` output (remove SSIDs you do not want to share). Do **not** attach project files containing other people's networks
without checking. Security problems: see [SECURITY.md](../../SECURITY.md).
