A professional-grade system monitor for Windows — deep hardware metrics, a game
overlay, and exportable reports.

![dashboard](https://raw.githubusercontent.com/lu020218/SysScope/master/docs/images/dashboard.png)

## New in 0.3.1

**Windows Defender no longer quarantines SysScope's driver.** Temperatures, package
power and fan speeds are read through a kernel driver. Until now that driver was
WinRing0, which Defender removes on sight as `VulnerableDriver:WinNT/Winring0` —
and it is right to: WinRing0 hands any process on the machine unrestricted MSR,
port and physical-memory access. When it got quarantined, temperature and power
silently turned into N/A, which is easy to mistake for a hardware fault.

SysScope now uses [PawnIO](https://pawnio.eu/) instead. It runs sensor routines as
sandboxed bytecode inside a signed driver, so the kernel only executes the specific
verified operations a sensor needs, and SysScope no longer writes a `.sys` file to
disk at all.

The trade-off is that PawnIO is installed separately — install it once from
[pawnio.eu](https://pawnio.eu/) and temperature, power and fan readings come back.
Without it, those read N/A and everything else works as before: GPU telemetry,
SSD health, the FPS overlay and every load and rate metric are unaffected.

SysScope now says so rather than leaving you to guess. If the driver is missing, a
banner names what is unavailable and links to the installer, and the Hardware tab
carries a **Sensor driver** row — so a missing driver is distinguishable from a
sensor your board simply does not have. Silence was the actual bug here: a
quarantined driver and a board without fan headers looked identical.

**Per-core clocks are back.** The new LibreHardwareMonitor renames hybrid-CPU clock
sensors from `CPU Core #n` to `P-Core #n` / `E-Core #n`, which emptied the per-core
frequency grid on Intel 12th gen and later. **CPU package power no longer reads
`0 W`** when the driver is unavailable — the sensor exists but reports zero, and a
running CPU drawing exactly zero watts is not a reading, it is a missing one.

If Defender already quarantined `sysscope.sys` from an earlier version, nothing
needs to be restored — that file is gone for good, and this version never creates
it.

## New in 0.3.0

**Three tabs instead of one crowded grid.** The window now has *Dashboard*,
*Processes* and *Hardware*. Processes moved out of the metric grid — where it
only had room for a top 5 — into its own page showing the top 10 by CPU, memory,
disk, network and GPU. Hardware moved out of a height-constrained dialog into a
full page.

**Threshold alerts.** When a metric stays over its threshold long enough
(15 s by default, configurable), SysScope raises a system notification and
records the event in the session, so exported reports carry an alert timeline.
The check runs in the sampling thread rather than the UI — the panel is usually
tucked in the tray when something goes wrong, and a hidden WebView gets throttled
by the browser engine. A hysteresis band keeps a metric hovering at its threshold
from firing repeatedly.

**Session comparison.** Tick two recorded sessions and get a report with their
curves overlaid and a delta table. Curves align on time *elapsed within each
session*, because two runs start at different moments — and gaps stay gaps, since
resampling would invent data points that were never measured. Deltas are shown
with sign and percentage but no red/green: a higher frame rate is good, a higher
temperature is not, and the report does not presume to know which you meant.

**Motherboard sensors.** Chassis fan speeds and board temperature points, read
from the SuperIO chip — useful for answering whether a fan has stopped or whether
VRM heat explains a CPU that keeps dropping clocks.

## Fixed: SysScope was using twice the CPU it claimed

Earlier versions documented "~20 ms per tick". The real figure was ~400 ms,
because reading temperatures through LibreHardwareMonitor pins the sampling
thread to each core in turn to read per-core MSRs — ~310 ms for 119 sensors on a
20-core part, and **the cost scales with core count**.

The wrong number survived because the benchmark that produced it never loaded the
sensor bridge in release builds, and because the true cost — 1.4% of a 28-thread
machine — is invisible in Task Manager. On a 4-thread laptop it was closer to 10%.

Expensive collectors now run on their own cadence: temperatures and power every
2 s, SMART every 10 s, fan speeds every 5 s. A typical tick is back to ~13 ms and
total CPU use is roughly halved. Temperature refreshes every 2 s instead of every
1 s — a deliberate trade, since temperature changes on a slower physical timescale
than a monitor burning a third of a core can be justified.

## What it does

Six subsystems, each with a live chart and a detail tab that answers *why*:

- **CPU** — per-core load, P/E-core grouping, package power, core voltage,
  effective clock, C-states, thermal-throttle flag
- **GPU** — clocks, hotspot & VRAM temps, power vs. limit, fan RPM,
  memory-controller load, encode/decode engines, PCIe throughput,
  hardware throttle reasons
- **Memory** — commit charge, standby/modified lists, hard page faults,
  compression, DIMM speed
- **Disk** — IOPS, sub-millisecond response times, queue depth, SSD health/TBW
- **Network** — latency with jitter and loss, TCP states, retransmit rate,
  link utilisation
- **Motherboard** — chassis fan RPM and board temperature points

**Hardware inventory** — CPU stepping, cache and microcode; every DIMM with part
number and slot; GPU VBIOS and PCIe link width; disk media, bus and firmware;
physical network adapters. One click copies it all as text for a bug report.

**FPS overlay** — follows the foreground app, shows frame rate plus 1% / 0.1%
lows, frame-time percentiles and stutter counts (ETW, same source as PresentMon).

**Recording & reports** — capture a session to SQLite, export as a self-contained
interactive HTML report, raw CSV/JSON, or a Markdown summary.

## Performance

| | |
|---|---|
| Typical tick | ~13 ms |
| CPU use, all in | ~20% of one core (0.8% of a 28-thread machine) |
| Startup to visible | 0.16 s |
| Memory (tray-resident) | ~140 MB |

## Install

Download the `.msi` below. Windows 10/11 x64.

## Before you start

- **Administrator rights are required.** The app requests elevation on launch —
  FPS capture needs an ETW kernel session and temperatures need a kernel driver.
  Everything else still works if you decline, but those metrics read N/A.
- **CPU temperature, package power and fan speeds need [PawnIO](https://pawnio.eu/)**,
  a small signed kernel driver installed separately. Without it those readings show
  N/A; the Hardware tab tells you whether it is installed.
- There is no auto-start toggle: Windows blocks elevated apps from normal startup
  entries. Use Task Scheduler with "run with highest privileges" and the
  `--minimized` flag if you want it at boot.
- Reports are written to `Documents\SysScope\reports`, with serial numbers and MAC
  addresses masked to their last four characters. Settings → Privacy turns full
  values back on.
- Full GPU telemetry needs an NVIDIA card; AMD/Intel GPUs report load and VRAM.
- SSD SMART data may be hidden by Intel RST/VMD.
- Motherboard sensors need a SuperIO chip LibreHardwareMonitor recognises; the
  card hides itself when nothing is reported.
- The UI ships in English and Simplified Chinese, following your Windows display
  language by default (Settings → Language to change it).

## Notes

Built with Rust + Tauri 2. See the
[README](https://github.com/lu020218/SysScope#readme) for details and
[ARCHITECTURE.md](https://github.com/lu020218/SysScope/blob/master/docs/ARCHITECTURE.md)
if you want to hack on it.

MIT licensed. Bundles LibreHardwareMonitor (MPL-2.0) — see NOTICE.
