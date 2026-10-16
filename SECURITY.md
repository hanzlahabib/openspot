# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for a security problem.

Use GitHub's private vulnerability reporting for this repository: **Security tab > Report a vulnerability**.
If that option is not available, open a public issue that says only that you have a security report and asks
for a private channel (no details), and a maintainer will arrange one.

Helpful details: the OpenSpot version, your operating system, what you did, what you expected and what
happened; for file-format problems, a minimal file that triggers it. Please do not include real SSID or BSSID
lists of other people's networks.

OpenSpot is maintained by volunteers. There is no guaranteed response time, but reports are read and
taken seriously, and you will be credited in the fix unless you prefer not to be.

## Supported versions

There has been no release yet. Once there is, **only the latest released minor version** (and the `main`
branch) receives security fixes. Pre-1.0 versions may change file formats and behaviour between minor
versions.

## What is in scope

OpenSpot reads data it does not control, so these are the interesting surfaces:

- **802.11 beacon contents** (the information-element parser, `openspot-ie`). Beacons come from any device in
  radio range. The parser is a pure function that has a fuzz target (`fuzz/`); a crash, hang or memory
  problem triggered by a crafted beacon is in scope.
- **Project files (`.ospot`, SQLite) and archives (`.ospotz`, zip).** Opening a project or importing an archive
  from someone else. Archive import validates entry names, sizes, checksums and the database header and is
  designed to leave the destination untouched on failure; bypasses are in scope.
- **Images and PDFs used as floor plans**, and **CSV import** of aliases.
- **The update check**, and the **speed tests** (HTTP requests to a URL the user enters; `iperf3` run as a
  child process with arguments built from the user's settings).
- **Preferences**, which are read from a user-writable file.

Out of scope: problems that require an attacker who already has your user account or can modify the
installation directory; the behaviour of the operating system's Wi-Fi stack, drivers or third-party tools
(`iperf3`); social engineering.

## Things to know about how releases are protected

- **The checksum in an archive and the `SHA256SUMS` file are not signatures.** They detect corruption and
  casual changes. Anyone who can replace a download can replace the checksum file next to it. Do not treat a file
  that verifies against a checksum from the same place as authentic.
- **Release binaries are not code-signed yet**, so Windows SmartScreen will warn about them. Signing is described
  in `packaging/windows/SIGNING.md` and has not been set up. Build from source if in doubt.
- **The release pipeline has never run** and the update check points at a placeholder repository. See
  `docs/guide/11-limits.md`.

## Privacy

OpenSpot has **no telemetry, no analytics and no accounts**. Scan results, projects and aliases stay on your
computer. The only network traffic it can create is:

- the **update check**, which is **opt-in** (off by default): one request to GitHub's public releases API,
  sending nothing about you or your networks beyond what any HTTPS request reveals (your IP address and
  the program's name);
- **speed tests you configure**, to servers you enter.

If you find the application contacting anything else, treat it as a vulnerability and report it.
