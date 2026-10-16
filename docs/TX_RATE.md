# Wireless transmit-rate map (H-10)

**Survey > metric > "Transmit rate (this computer's connection)"** colours the plan by the
negotiated transmit rate (Mbps, higher is better) that the adapter reported while it was
connected at each measurement point.

## What it shows, and what it does not

* It is the rate of the **laptop's own link** to the network it was associated with. It is not a
  property of every access point that was heard, and it says nothing about APs the laptop was
  not connected to. In a multi-AP site it shows how well the laptop's connection performed
  as it moved and roamed, with the connected BSSID kept per point (CSV column
  `connected_bssid`).
* The rate is what rate adaptation had chosen at that moment, so it fluctuates between two
  scans taken at the same spot. With several scans per point, the value from the last scan is
  recorded (as with active-scan results).
* The map is a **client-side** quantity: another device in the same place (different antennas,
  band support or power) would get different rates.

## What gets recorded

`ScanPoint::link` (`openspot_model::LinkInfo`): connected BSSID, transmit rate and receive rate in
Mbps. The survey worker reads `WlanProvider::current_connection` right after each survey scan.
A point has no link when the adapter was not associated (state other than *connected*), the
platform cannot report a connection (macOS and Linux backends are unverified), or reading it
failed; the measurement itself is never failed over this.

A rate of 0 means the driver reported none. Such points and points without a link contribute no
sample: like the speed maps, the map interpolates between points that actually have a rate and
does not treat unknown as slow.

Links are stored in the project file (migration slot 6, table `scan_link`); files saved by older
versions load without links.

## Legend

Red (0 Mbps) through orange (50), yellow (200) and light green (600) to green (1200 Mbps and
above). 1200 Mbps is the 2x2 80 MHz Wi-Fi 6 maximum; use the legend's value range (H-15) to
rescale for other hardware.

## CSV

`snapshot_to_csv` appends `connected_bssid,tx_rate_mbps,rx_rate_mbps` to every row. The same link
repeats on each access-point row of a point. Empty cells mean unknown; they are never written
as 0.
