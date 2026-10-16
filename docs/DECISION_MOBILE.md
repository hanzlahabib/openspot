# Decision record: Android and iOS (S-12)

Status: decided, 2026-10-16. Android and iOS are **out of scope for 1.0**.

## Context

OpenSpot's value is passive, per-BSSID scanning (RSSI, channel, IE-derived PHY and
security) walked through a floor plan. The platform APIs decide whether that is possible.

## Findings

* **iOS** exposes no public API to scan for nearby networks. `NEHotspotHelper` needs an
  Apple-granted entitlement and only reports networks to a hotspot-helper app;
  `CNCopyCurrentNetworkInfo` returns only the connected network (and needs location
  permission). A site-survey tool cannot be built on public iOS APIs.
* **Android** has `WifiManager.startScan()`/`getScanResults()` with BSSID, RSSI, frequency
  and raw IEs (API 30+ `ScanResult.getInformationElements()`), but since Android 9 scans
  are throttled (4 per 2 minutes per app in the foreground, 1 per 30 minutes in the
  background) and need location permission plus location services on. Developer options
  can lift the throttle. This is workable for a survey but needs a second UI toolkit and a
  JNI backend; the egui desktop app does not carry over.

## Decision

1. No Android/iOS work in 1.0. The desktop targets (Windows, Linux, macOS) are the product.
2. The `WlanProvider` trait stays platform-neutral so an Android backend (JNI to
   `WifiManager`) can be added later without touching the model, heatmap, or export crates.
3. If mobile is revisited, Android comes first, as a data-collection companion that writes
   the `.openspot` project format (or CSV import, I-12) for the desktop app to visualise.
   iOS is not feasible with public APIs.
