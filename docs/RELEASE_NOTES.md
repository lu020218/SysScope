A professional-grade system monitor for Windows — deep hardware metrics, a game
overlay, and exportable reports.

![dashboard](https://raw.githubusercontent.com/lu020218/SysScope/master/docs/images/dashboard.png)

## New in 0.2.0

**The UI speaks English now.** Everything — panels, dialogs, tray menu, overlay
and exported reports — ships in English and Simplified Chinese. It follows your
Windows display language on first run; Settings → Language switches it. CSV and
JSON exports keep English column names either way, so scripts that read them
don't break.

**Hardware inventory.** A new panel tells you what the machine actually is,
beyond what it is doing right now:

- **CPU** — socket, cache sizes, stepping, microcode revision, instruction sets,
  virtualisation state
- **Memory** — every DIMM with part number, rated vs. configured speed, and which
  slot it occupies, plus how many slots are free
- **Graphics** — driver and VBIOS versions, PCIe link gen and width (current vs.
  maximum), true VRAM size
- **Storage** — media type, bus, firmware revision, SMART health per drive
- **Network** — physical adapters only, with MAC, addresses, gateway, DNS and
  negotiated link speed
- **Motherboard / BIOS / OS** — board model and revision, BIOS version and date,
  build number, install date, uptime

One button copies the whole thing as text, which is what you want when filing a
bug report. Reports embed it too, so a performance capture carries the
configuration it was taken on.

**Privacy for exported reports.** Serial numbers and MAC addresses show in full
in the app, but reports keep only their last four characters — reports get
shared, and those values identify your machine. Settings → Privacy re-enables
full values when you actually need them.

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
- **Processes** — top 5 by CPU, memory, disk, network **and GPU**; click a row
  for threads, handles, private bytes, page faults, affinity

**FPS overlay** — follows the foreground app, shows frame rate plus 1% / 0.1%
lows, frame-time percentiles and stutter counts (ETW, same source as PresentMon).

**Recording & reports** — capture a session to SQLite, export as a self-contained
interactive HTML report, raw CSV/JSON, or a Markdown summary.

## Performance

| | |
|---|---|
| Sampling cost | ~20 ms per tick |
| Startup to visible | 0.16 s |
| Memory (tray-resident) | ~140 MB |

The hardware inventory is collected once on first use (~320 ms) and cached — it
never touches the sampling loop.

## Install

Download the `.msi` below. Windows 10/11 x64.

## Before you start

- **Administrator rights are required.** The app requests elevation on launch —
  FPS capture needs an ETW kernel session and temperatures need a kernel driver.
  Everything else still works if you decline, but those metrics read N/A.
- There is no auto-start toggle: Windows blocks elevated apps from normal startup
  entries. Use Task Scheduler with "run with highest privileges" and the
  `--minimized` flag if you want it at boot.
- Reports are written to `Documents\SysScope\reports`.
- Full GPU telemetry needs an NVIDIA card; AMD/Intel GPUs report load and VRAM.
- SSD SMART data may be hidden by Intel RST/VMD.

## Notes

Built with Rust + Tauri 2. See the
[README](https://github.com/lu020218/SysScope#readme) for details and
[ARCHITECTURE.md](https://github.com/lu020218/SysScope/blob/master/docs/ARCHITECTURE.md)
if you want to hack on it.

MIT licensed. Bundles LibreHardwareMonitor (MPL-2.0) — see NOTICE.
