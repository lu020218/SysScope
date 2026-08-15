A professional-grade system monitor for Windows — deep hardware metrics, a game
overlay, and exportable reports.

![dashboard](https://raw.githubusercontent.com/lu020218/SysScope/master/docs/images/dashboard.png)

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
- UI language is Simplified Chinese.

## Notes

Built with Rust + Tauri 2. See the
[README](https://github.com/lu020218/SysScope#readme) for details and
[ARCHITECTURE.md](https://github.com/lu020218/SysScope/blob/master/docs/ARCHITECTURE.md)
if you want to hack on it.

MIT licensed. Bundles LibreHardwareMonitor (MPL-2.0) — see NOTICE.
