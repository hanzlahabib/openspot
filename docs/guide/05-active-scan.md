# 5. Active scan

A passive scan only listens. An **active scan** adds a speed test at each survey point, so you can
map what a client actually gets, not just what it hears. It is optional and off by default.

**Status:** implemented and covered by tests against local servers, but **never run against a real
iperf3 server, a real internet server or the Wi-Fi network switching on hardware**. Treat the first
real use as a trial and compare against a tool you trust. See [Limits](11-limits.md).

## What it measures

| Test | What happens |
|---|---|
| HTTP download | Downloads from a URL you provide, up to the size you set, and divides bytes by time from the first response byte to the end |
| HTTP upload | Posts a generated payload to a URL you provide and times it until the server answers |
| iperf3 TCP download / upload | Runs the system `iperf3` as a client against your server (download uses iperf3's reverse mode) |
| iperf3 UDP download / upload, jitter | The same, over UDP at a bitrate you choose, also reporting jitter |

Throughput is in megabits per second (10^6 bits). Each becomes its own map in the metric picker
once you have measured points: see [Active scan maps](04-heatmaps.md#active-scan-maps).

OpenSpot sends **nothing** anywhere until you enter a server. The URL fields are empty by default and
no third-party speed-test service is built in.

## Setting up iperf3

1. **Install iperf3 on the survey computer.** Linux: `sudo apt install iperf3` (or your
   distribution's equivalent). macOS: `brew install iperf3`. Windows: download an iperf3 build for Windows and put
   `iperf3.exe` in a folder on your `PATH`, or enter its full path in **iperf3 program** in the settings
   (leave it empty to search `PATH`).
2. **Run an iperf3 server somewhere on the wired side of your network**, ideally a PC connected to the
   router by cable so that the Wi-Fi link is the slow part:

   ```sh
   iperf3 -s
   ```

   It listens on port 5201 for TCP and UDP. Allow that port in the server's firewall.
3. In Survey, open **Active scan settings...**, tick **iperf3 TCP** and/or **iperf3 UDP**, and enter the
   server's address as **iperf3 server** (and the **Port** if it is not 5201).
4. Press **Test connection**. Green lines mean the program was found and the server answered; red lines
   say what failed.

Settings worth knowing:

| Setting | Default | Notes |
|---|---|---|
| Duration | 10 s | Per test and direction. At each survey point you stand still for the sum of all tests |
| Parallel streams | 1 | More streams can fill a fast link; 128 at most |
| Window | 0 (OS default) | TCP window in KiB |
| UDP rate | 50 Mbit/s | 0 means unlimited. A rate far above what the link carries makes the loss and jitter numbers extreme |
| Send UDP at the TCP result | on | Uses the measured TCP rate of the same direction for the UDP test when TCP ran |
| Delay before testing | 2 s | Pause after (re)connecting so DHCP settles |

## Setting up the HTTP test

Tick **HTTP**, then give:

- **Download URL**: any URL that serves at least the size you set (default 25 MB). The download stops
  after that many bytes, so a larger file is fine.
- **Upload URL**: a URL that accepts a POST body of the size you set (default 10 MB) and answers when done.
  Few public servers do; running one on your own network is typical.
- **HTTP timeout**: the whole request is abandoned after this (60 s by default).

HTTP results include internet routing, the server's speed and your uplink, not just Wi-Fi. For a Wi-Fi survey,
iperf3 to a server on your own LAN isolates the radio far better.

## Running it

Tick **Active scan** in the Survey toolbar, then measure as usual: each click (or path sample) first does the
normal passive scan and then runs the chosen tests. Progress appears in the status bar with a **Cancel**
button; cancelling stops the speed tests and keeps the passive scan.

**Testing several networks (Windows only).** Enter saved Wi-Fi profile names (usually the SSID), one
per line, under *Networks to test*. At each point OpenSpot connects to each in turn, runs the tests,
and restores the connection you had before, even if a test fails or is cancelled. This needs saved
profiles, switches your computer's connection while it runs (so do not rely on the connection
for anything else), and has not been verified on hardware. Leave the list empty to test only the network you
are already connected to.

Failures are recorded as errors with the measurement point instead of disappearing, and a point
without a result for a test is left out of that map rather than shown as zero.

## If it does not work

- "could not start iperf3 (is it installed and on PATH?)": install it or set the program path.
- iperf3 "unable to connect to server": wrong address or port, server not running, or a firewall.
- A Download test returns "server answered HTTP 404" or similar: the URL is wrong for that server.
- Results of exactly zero or "transfer too short": the transfer finished before a measurable time passed; use a larger size.
