# Code signing and SmartScreen (Windows)

**Status: not set up. Nothing here has been run.** The notes below are written from the public
documentation as understood when this was written; check Microsoft's current pages before you spend money
or time, because the signing services and SmartScreen policy change.

## Why sign

An unsigned `.exe` or installer downloaded from the internet triggers Windows **SmartScreen**
("Windows protected your PC") and shows "Unknown publisher" in the UAC and file-properties dialogs. Signing
replaces "Unknown publisher" with the publisher name on the certificate. It does **not**, by itself, make
SmartScreen go quiet.

## SmartScreen reputation

SmartScreen judges a download by the signer's and the file's reputation, which accumulates as many
people download and run it without trouble. A newly signed file from a new publisher can still be
warned about for a while; an unsigned one is warned about too. Practical consequences:

- Do not promise users a warning-free first install. Tell them what the dialog means and how to proceed
  (**More info > Run anyway**) in the release notes.
- Sign **consistently** with the same identity so reputation is not reset by each release.
- Report false positives through Microsoft's file-submission portal for software publishers.
- Whether an extended-validation (EV) certificate or a newer service gives an instant reputation head start has changed over
  time; verify the current policy rather than assuming it.

## Options

### 1. A certificate from a certificate authority (signtool)

Buy a code-signing certificate (OV or EV) from a CA. Since 2023 the private key must live on a
hardware token or in a cloud HSM, so you cannot simply keep a `.pfx` in CI.

```powershell
signtool.exe sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /a <file>
signtool.exe verify /pa /v <file>
```

`/tr` with `/td SHA256` adds an RFC 3161 **timestamp**, which keeps the signature valid after the certificate expires.
Pick the timestamp server your CA documents.

### 2. Azure Trusted Signing

A Microsoft service that signs with short-lived certificates and an identity you validate once; no hardware token to manage, and it works
from GitHub Actions. Microsoft has been renaming the product, so search for its current name. Outline:

1. Create the signing account and a certificate profile in Azure; complete identity validation (eligibility depends on
   the kind of organisation and country, so check it first).
2. Install the client tooling (a recent `signtool.exe` plus the Azure signing dlib) and a metadata file with the endpoint, account and profile.
3. Sign with `signtool.exe sign /v /fd SHA256 /tr http://timestamp.acs.microsoft.com /td SHA256 /dlib <path-to-Azure.CodeSigning.Dlib.dll> /dmdf <metadata.json> <file>`.
4. In CI, authenticate with a service principal or workload identity (secrets in the repository's encrypted secrets, never in the repository).

## What to sign, in order

1. `openspot.exe` and `openspot-gui.exe` (sign before packaging, so the signed binaries go into both the zip and the installer).
2. The installer: `build-installer.ps1 -SignCommand '<signtool command with $f for the file>'` makes Inno Setup sign the installer and the
   uninstaller (`SignedUninstaller=yes`).
3. Regenerate `SHA256SUMS` after signing (the build scripts do this; signing changes the bytes).

The portable zip itself is not signed; the executables inside are.

## Not a replacement for checksums, and vice versa

`SHA256SUMS` lets someone confirm a download matches what was published **if** they got the checksum file from a place an attacker cannot
also alter. It is **not a signature**: anyone who can replace the zip can replace the checksum file next to it. See
[SECURITY.md](../../SECURITY.md).

## Verifying on a clean machine

After the first signed build, on a Windows machine that never saw the file: download through a browser (so the Mark of the Web is set),
run it, and note what SmartScreen and UAC say. Record the result in `docs/guide/11-limits.md`.
