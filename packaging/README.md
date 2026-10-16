# Packaging

Everything here is **UNVERIFIED unless stated otherwise**. The development environment is Linux/WSL
with no Windows machine, no Inno Setup, no Mac and no signing certificate.

| Artifact | Files | State |
|---|---|---|
| Windows portable zip + `SHA256SUMS` | `windows/build-portable.ps1`, `windows/build-portable.sh` | The bash version is tested here with dummy binaries (`tests/portable-zip.test.sh`: zip contents, entry names, checksums). The PowerShell version is parsed and run under PowerShell 7 on Linux by the same test when `pwsh` is installed; **not run on Windows PowerShell 5.1**. Real release binaries were not used |
| Windows installer (Inno Setup 6) | `windows/openspot.iss`, `windows/build-installer.ps1` | **Never compiled.** The PowerShell wrapper is syntax-checked only. The `.iss` was reviewed against the Inno Setup directive documentation by hand |
| Code signing | `windows/SIGNING.md` | Notes only. No certificate, no signing run |
| Linux AppImage | `linux/` | EXPERIMENTAL sketch, never run. Needs an icon the repository does not have |
| macOS `.app` | `macos/` | EXPERIMENTAL sketch, never run |

## Windows

Build the release binaries first.

```powershell
cargo build --release --target x86_64-pc-windows-msvc -p openspot-cli -p openspot-app

# Portable zip + SHA256SUMS in dist\
powershell -ExecutionPolicy Bypass -File packaging\windows\build-portable.ps1

# Installer (needs Inno Setup 6); also refreshes SHA256SUMS
powershell -ExecutionPolicy Bypass -File packaging\windows\build-installer.ps1
```

From Linux or WSL, with the `x86_64-pc-windows-gnu` cross build, the zip can be made without PowerShell:

```sh
cargo build --release --target x86_64-pc-windows-gnu -p openspot-cli -p openspot-app
packaging/windows/build-portable.sh                 # needs zip and sha256sum
```

The version in every file name comes from `[workspace.package]` in `Cargo.toml`, the same place the
release workflow reads it from.

### What the installer does

- **Per-user install** into `%LOCALAPPDATA%\Programs\OpenSpot`; no administrator prompt.
- Start Menu entry; an optional desktop shortcut.
- Uninstaller registered in "Installed apps" (removes the program, shortcuts and file-type keys).
  Your preferences (`%APPDATA%\OpenSpot\prefs.json`) and projects are **left in place**.
- Optional registration of the `.ospot` and `.ospotz` file types. It is **off by default** because the
  application does not yet open a project passed on the command line: double-clicking a file would start
  OpenSpot but not load the file. The registration (icon, "open" verb) is there ready for when that is wired up.
- No application icon is embedded; the program uses the default icon. There is none in the repository.

### Portable zip

Contains `openspot-gui.exe`, `openspot.exe`, README, CHANGELOG and both licences under one top-level
folder. "Portable" means no installer; settings are still written to `%APPDATA%\OpenSpot`.

## Checksums

`SHA256SUMS` is in `sha256sum` format, so on any platform: `sha256sum -c SHA256SUMS`
(or `shasum -a 256 -c SHA256SUMS` on macOS; on Windows `Get-FileHash -Algorithm SHA256 <file>` and compare).

**A checksum file is not a signature**: it proves a file matches the checksum, not who produced either.
See [SECURITY.md](../SECURITY.md) and [windows/SIGNING.md](windows/SIGNING.md).

## Tests

```sh
packaging/tests/portable-zip.test.sh
```

Builds a dummy release folder, runs `build-portable.sh` (and `build-portable.ps1` if `pwsh` is on `PATH`), then checks
the zip's entries (one top-level folder, the expected files, no backslashes, no extras), that extracted
binaries are byte-identical to the inputs, that `SHA256SUMS` verifies with `sha256sum -c` and that a
tampered file fails the check. It does not run in CI yet (the workflows are owned by the lead); add
`run: packaging/tests/portable-zip.test.sh` to a Linux job to enable it.
