# 11. Limits and known unverified areas

This page lists what is not finished, not verified, or deliberately not done. It follows
[ROADMAP.md](../ROADMAP.md), which is the authority and changes faster than this page; if they disagree, the
roadmap is right. If something is not on this page it is not a promise that it works; "done" in the roadmap
means implemented and tested in the repository's environment (Linux and WSL, no radio, no display).

## How things were tested

- Automated tests: parsers, interpolation, heatmap rendering (golden images), the project store, reports,
  archives, preferences, localization, and most application logic.
- Real hardware: **one Windows 11 machine with an Intel AX211 adapter**. Scanning, the connection
  information and the noise-floor research were checked there.
- **Never seen by a person on a screen** during development of this build: the layout of the application windows and the
  appearance of PDF reports. Automated checks passed; look yourself.

## Platforms

| Platform | State |
|---|---|
| Windows 10/11 | Scanning verified on one adapter. Everything else in this guide applies |
| Linux | A scanner using `nl80211` exists and compiles; **unverified on hardware**. Scanning needs the `CAP_NET_ADMIN` capability. The GUI builds in CI on Linux; it has not been used to survey |
| macOS | A CoreWLAN scanner exists and is **type-checked only, never run**. The release workflow builds it, unverified |
| Android, iOS | Out of scope for 1.0 ([DECISION_MOBILE.md](../DECISION_MOBILE.md)) |

## Scanning and Inspector

- Hot-plug and power notifications are implemented in the library but **not wired into the application**, and no event
  has been observed on hardware. Changing adapter state while running may need a restart.
- Vendor lookup covers only the 24-bit (MA-L) IEEE assignments; devices in longer blocks show no vendor.
- Sound notifications work on Windows only.
- There is **no noise floor** on Windows, so no SNR or noise map ([why](09-troubleshooting.md#why-are-there-no-snr-or-noise-maps)).
- Aliases are application-wide only; per-project aliases are not implemented.

## Survey

- Network selection and aliases work in Survey; custom groups are not available there (roadmap V-11, partial).
- Undo and redo cover points, scale and merges only. Zone and snapshot changes and display settings cannot be undone.
- A sound on measurement success or failure is not implemented.
- The **wireless transmit rate** map is not available: the rate is not recorded at each point.

## Active scan

HTTP and iperf3 tests, and switching between networks, are implemented and tested against local fake servers only.
**Unverified**: a real iperf3 server, real internet servers, and connecting to networks and restoring the
connection on hardware. See [Active scan](05-active-scan.md).

## Reports and archives

- PDF structure, text and images are verified by tests; **page appearance is unverified**.
- Non-Latin, Greek and Cyrillic text prints as replacement characters; right-to-left text is not reordered.
- The archive checksum is not a signature.
- Importing survey files from other products is **out of scope** for clean-room reasons.

## Planning

The calculation engine is done; the **interface is being built**, and router and vendor antenna catalogues are
not provided. Predictions have not been validated against a real building. See [Planning mode](10-planning.md).

## Validation of numbers

The real-world validation corpus (captured beacons and comparison against a trusted tool at the same spots) is **not done**
(roadmap Q-01). Until it is, treat absolute numbers, the default troubleshooting thresholds and the AP position
estimates as unvalidated. Relative comparisons (before and after a change, with the same method at the same spots) are the sound way to use a survey.

## Distribution

- The **release workflow** has never run on GitHub.
- The **Windows installer and portable zip** scripts in `packaging/` have not been run on Windows; **code signing is not set up**, so Windows will warn
  about an unrecognised publisher (SmartScreen).
- The **update check** points at a placeholder repository and cannot find releases yet.
- Linux AppImage and macOS app bundle files are experimental sketches.

## Localization

Only English ships. The translation machinery works and covers the mode bar and the Preferences window; everything else is
English text in the code. See [I18N.md](../I18N.md).

## Accessibility

Keyboard navigation, a high-contrast theme and larger interface sizes exist. The application has not been tested with a screen reader.
