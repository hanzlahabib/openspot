# macOS packaging (EXPERIMENTAL, UNVERIFIED)

Nothing here has been run. The macOS Wi-Fi backend (CoreWLAN) is only type-checked; no one has run
OpenSpot on a Mac. Treat these notes as a starting point for whoever first does.

## What the bundle needs

`build-app-bundle.sh` creates `dist/OpenSpot.app` from a release build of `openspot-gui` and the
`Info.plist.template` in this folder (version filled in from `Cargo.toml`).

- **Location Services.** From macOS 14, CoreWLAN redacts SSID and BSSID unless the app is authorised for
  Location Services. The backend notices (it reports "access denied"), but the application does not
  *request* the authorisation itself (that needs `CLLocationManager`), so a first run is likely to show
  nothing useful until someone adds that. The Info.plist carries the usage descriptions the prompt needs. See also
  [NOISE_FLOOR.md](../../docs/NOISE_FLOOR.md) for what CoreWLAN does and does not expose.
- **Icon.** No `.icns` exists in the repository.
- **Architectures.** The release workflow builds `aarch64-apple-darwin` only. An Intel build is a second
  target; combine the two with `lipo -create -output openspot-gui a b` for a universal binary.

## Signing and notarization (needed to distribute)

An ad-hoc signature (what the script applies) runs on your own Mac only. For a download that opens without
warnings you need:

1. An Apple Developer Program membership and a **Developer ID Application** certificate.
2. `codesign --force --options runtime --timestamp --sign "Developer ID Application: <name> (<team>)" OpenSpot.app`
   (the hardened runtime is required for notarization).
3. Submit with `xcrun notarytool submit OpenSpot.zip --keychain-profile <profile> --wait`, then
   `xcrun stapler staple OpenSpot.app`.
4. Package as a `.dmg` or `.zip` (signed `.dmg`s can be notarized too).

Check Apple's current documentation before relying on these steps; they were written from memory of the
public workflow and are unverified.
