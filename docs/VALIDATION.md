# Validation against Windows (Q-01)

`openspot verify` scans with OpenSpot and compares the result, network by network, with what
Windows itself prints for the same networks (`netsh wlan show networks mode=bssid`). It is
read-only: one scan, one `netsh wlan show`, nothing that connects, disconnects or edits Wi-Fi.

## What it validates, and what it does not

**It validates** that OpenSpot's decoding of the data Windows hands over agrees with Windows' own
presentation of the same data, per BSSID:

* primary **channel** number,
* **band** (2.4 / 5 / 6 GHz),
* **radio type**: the newest 802.11 generation OpenSpot reads from the beacon's capability
  elements against netsh's "Radio type",
* **signal**: OpenSpot's RSSI (dBm) against netsh's link-quality percentage converted to dBm,
* the networks seen by only one side.

**It does not validate** (these need a ground-truth survey, which this is not):

* The radio. Both sides read the **same Windows BSS list**, so a driver that misreports
  channels or RSSI would be wrong in both. Agreement says nothing about the adapter's accuracy.
* Heatmap accuracy: interpolation, smoothing, categorical maps, contours and AP-position estimates
  are not exercised at all. Checking them needs measurements at known positions compared with an
  independent reference (a second tool on the same spots, or a dense hand-checked grid).
* Prediction accuracy (propagation model, wall attenuation, calibration): same, needs measured
  ground truth at the predicted spots.
* Anything the beacon decoding gets wrong *consistently with Windows*, for example a PHY that
  neither side recognises.
* Security, vendor, width and other columns. They are not in netsh's output.

So it is a regression and sanity check for the scan path (parser, band and channel
arithmetic, RSSI), not a certificate of survey accuracy.

## How the comparison is made

1. `openspot verify` scans (3 merged probes by default) so that Windows' BSS list is fresh.
2. It runs `netsh wlan show networks mode=bssid` (or reads `--netsh-file`).
3. Entries are matched by BSSID. Fields a side did not provide are "not comparable", never a
   mismatch.

### Signal

netsh does not print dBm. It prints Windows' signal quality in percent, which Windows documents
as a linear map of -100 dBm (0 %) to -50 dBm (100 %): `dBm = -100 + percent / 2` (the same
mapping `openspot_wlan::quality_to_dbm` uses for connection info). Two details matter:

* **The scale ends at -50 dBm.** A signal stronger than that still reads about 100 %, so a
  stronger OpenSpot RSSI is compared as -50 dBm (and one weaker than -100 as -100). Without
  this, the strongest AP of a survey is always "wrong" by however far it exceeds -50 dBm. This
  rule was added after the first real run, which showed exactly that (see Results); the
  threshold below was fixed beforehand and not changed afterwards.
* **Tolerance: 5 dB** (`SIGNAL_TOLERANCE_DB`). The percentage is an integer (0.5 dB per step), the
  driver's percentage only approximates RSSI, and the two numbers come from scans moments
  apart (OpenSpot's probe merge alone varies about 1 dB, receivers a few dB). 5 dB tolerates
  that but would still catch a sign error or a 10 dB offset. It is a reporting threshold, not
  an accuracy claim. The report also prints the mean absolute, maximum absolute and mean signed
  difference, so a systematic offset shows even when every network is inside the tolerance.

### Language

netsh translates its field names. The parser recognises the English names (`Signal`,
`Radio type`, `Band`, `Channel`) and nothing else:

* Entries with some English fields are compared on those; the others count as "not
  comparable" and the report says how many entries were incomplete.
* BSSID entries with **none** of the expected fields (a German or Japanese Windows, say) make
  the command fail with an explanation and the field names it saw, instead of reporting
  zeros or mismatches.
* Output with no recognisable entries at all fails as "nothing recognised" (the zero-network
  English message is the one accepted empty answer).

If you hit this, switch the Windows display language to English, or pass `--netsh-file` with output
captured under an English session.

## Repeating it

On Windows (or from WSL through interop with the Windows build):

```sh
cargo build --release --target x86_64-pc-windows-gnu -p openspot-cli
target/x86_64-pc-windows-gnu/release/openspot.exe verify            # text summary
target/x86_64-pc-windows-gnu/release/openspot.exe verify --json     # same, machine readable
target/x86_64-pc-windows-gnu/release/openspot.exe verify --strict   # exit status 3 on any difference
```

Run it several times, a few seconds apart, in the place you normally use the laptop; scan
results vary between runs. `--details` lists every network with its BSSID, which identifies
nearby access points, so keep that output private. The default output, and `--json` without
`--details`, contain only counts and statistics and are safe to paste into an issue.

Away from Windows, `openspot verify --fake --netsh-file saved.txt` runs the same comparison
against a saved netsh capture (the fake provider's networks will of course not match a real
capture; the option exists for tests and for checking a capture parses).

## Results (sanitized)

Environment: Intel AX211 (Wi-Fi 6E) on Windows 11, English display language, run through WSL
interop on 2026-10-16. No names, addresses or locations are recorded here. Each run lists the networks visible at that moment.

First run, before the quality-scale rule was added (4 networks):

| Networks (OpenSpot / netsh / matched) | Channel | Band | Radio type | Signal mean \|diff\| | max \|diff\| | within 5 dB |
|---|---|---|---|---|---|---|
| 4 / 4 / 4 | 4 match | 4 match | 4 match | 6.5 dB | 16 dB | 3 of 4 |

The one outlier was the strongest network (OpenSpot -36 dBm, netsh 96 % = -52 dBm): the
adapter's own RSSI was well above the end of the quality scale, so the 16 dB was an artefact of
the comparison, not of the scan. The other networks (-70 to -90 dBm) were within 2 dB.

Six further runs with the rule in place (6 networks in each, all matched by BSSID, none seen by
only one side):

| Run | Channel / band / radio type | Signal mean \|diff\| | max \|diff\| | mean signed | within 5 dB |
|---|---|---|---|---|---|
| 1 | 6/6, 6/6, 6/6 match | 2.0 dB | 3 dB | +1.0 | 6 of 6 |
| 2 | all match | 1.7 dB | 2 dB | +1.3 | 6 of 6 |
| 3 | all match | 1.7 dB | 2 dB | +1.3 | 6 of 6 |
| 4 | all match | 1.2 dB | 2 dB | +1.2 | 6 of 6 |
| 5 | all match | 1.8 dB | 3 dB | +1.8 | 6 of 6 |
| 6 | all match | 2.5 dB | 7 dB | +0.2 | 5 of 6 |

Over the six runs: 36 of 36 network comparisons agreed on channel, band and radio type; the mean
absolute signal difference was 1.8 dB overall and the worst single difference 7 dB (one network
in one run, which exceeded the 5 dB threshold; weak signals fluctuate between scans a few
seconds apart). The signed mean was +1.1 dB on average, i.e. OpenSpot reads very slightly
stronger than the percentage-derived value.

This is one adapter, one driver, one location and a handful of networks: enough to show the
scan path decodes and reports what Windows reports, not a statement about accuracy in general.
