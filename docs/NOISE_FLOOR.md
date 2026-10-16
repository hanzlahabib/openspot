# Noise floor source (S-07)

Status: researched on a real Intel AX211 (Windows 11), decision recorded. No noise value
is exposed, so `AccessPoint` has **no** `noise_dbm` field and SNR (H-04) / noise maps
(H-06) must not be fed from Windows scans.

## What Windows exposes

| Source | Result on the AX211 |
|---|---|
| `WLAN_BSS_ENTRY` (`WlanGetNetworkBssList`) | `lRssi`, `uLinkQuality`; no noise member. |
| `WlanQueryInterface` `wlan_intf_opcode_rssi` | Connected-link RSSI only (dBm). |
| `WlanQueryInterface` `wlan_intf_opcode_statistics` | Succeeded (1088 bytes, `WLAN_STATISTICS`). Only frame counters (tx/rx, FCS errors, ACK/RTS failures, retries, handshake failures). No noise or channel-utilisation field. |
| IHV opcode range (`0x30000000`+) | `ERROR_INVALID_PARAMETER` (87): the Intel driver registers no vendor opcodes. |
| `netsh wlan show interfaces` | Prints no noise line. |
| NDIS OIDs | `OID_DOT11_*` statistics are miniport-level; user mode reaches them only through the WLAN service (above) or a driver of our own. Not an option for an unprivileged tool. |

Conclusion: the Windows Native Wifi API has no noise-floor source. The documented
`wlan_intf_opcode_*` list contains nothing noise-related.

## Other platforms (UNVERIFIED here, from public docs)

* Linux `nl80211`: `NL80211_CMD_GET_SURVEY` returns `NL80211_SURVEY_INFO_NOISE` (dBm)
  per channel the radio has visited, together with channel active/busy time. Drivers
  differ in whether they fill it. This is a **per-channel** value and the best source we have.
* macOS CoreWLAN: `CWInterface.noiseMeasurement` is the noise of the **currently
  associated channel only**.

## Decision

1. Do not invent or estimate a noise number. A fixed "-95 dBm" would make SNR a
   re-labelled RSSI.
2. Keep `AccessPoint` unchanged. If a Linux/macOS per-channel noise reading is added
   later it belongs in a separate per-channel structure (e.g. `ChannelNoise { channel,
   noise_dbm }`) returned by a new provider method with an `Unsupported` default, not on
   each AP.
3. H-04 and H-06 should therefore present SNR/noise only when such a reading exists,
   and otherwise show RSSI-based metrics. A hardware-based path (external spectrum
   analyzer) is out of scope for 1.0.
4. A useful Windows-side proxy that *is* available is retry/ACK-failure counters from
   `wlan_intf_opcode_statistics`; they indicate a noisy channel on the connected link but
   are not dBm, so they must never be labelled as noise.

## Addendum: macOS

`CWNetwork.noiseMeasurement` (CoreWLAN) is documented as a per-network noise reading in
dBm. It is **not** wired into `AccessPoint`: the macOS backend is unverified at runtime,
and CoreWLAN returns 0 when no measurement exists, which cannot be told apart from a real
value without testing on hardware. Revisit together with the per-channel structure above
once someone can run the macOS backend.
