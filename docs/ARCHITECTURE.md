# Architecture

A map of the codebase for anyone who wants to change it.

## Shape of the app

```
                    ┌──────────────────────────────────────┐
                    │  sampling thread (one, owns all       │
                    │  collectors; panic-guarded)           │
                    │                                       │
   PDH ────────────▶│  take_snapshot() per tick (~13 ms)    │
   NVML ───────────▶│         │                             │
   ETW ────────────▶│         ├──▶ SQLite (when recording)   │
   LHM (2 s) ──────▶│         │                             │
   sysinfo/Win32 ──▶│         └──▶ emit("metrics", Snapshot)│
                    └──────────────────┬───────────────────┘
                                       │  Tauri event
                    ┌──────────────────▼───────────────────┐
                    │  main window  │  FPS overlay window   │
                    │  (cards)      │  (compact strip)      │
                    └──────────────────────────────────────┘
```

One thread does all collection and broadcasts a single `Snapshot` per tick. Both
windows consume the same event; nothing polls the backend for metrics. Commands
(`invoke`) are used only for actions — start recording, export a report, toggle
the overlay, query one process in depth.

## Backend (`src-tauri/src/`)

| File | Responsibility |
|---|---|
| `lib.rs` | Tauri setup, command registry, tray, window plumbing |
| `elevate.rs` | Self-elevation at startup (runs before Tauri initialises) |
| `i18n.rs` | The handful of native strings — tray, elevation dialog, reports |
| `hwinfo.rs` | Static hardware inventory — queried once, cached forever |
| `sampler.rs` | The sampling loop, `SamplerCtx`, `Snapshot` assembly, top-N processes |
| `pdh.rs` | PDH query wrapper — the single query handle every counter hangs off |
| `disk.rs` | Physical disk I/O, latency, volumes |
| `mem_ext.rs` | Commit charge, page faults, cache lists, DIMM info |
| `cpu_perf.rs` | Effective clock, C-states, base clock, P/E-core topology |
| `net_ext.rs` | TCP/UDP connection tables, retransmits, adapter utilisation |
| `gpu.rs` | GPU telemetry — NVML first, PDH counters as fallback |
| `gpu_proc.rs` | Per-process GPU usage and VRAM |
| `fps.rs` | ETW frame capture, foreground-window tracking, FPS statistics |
| `netproc.rs` | Per-process network bytes via ETW kernel provider |
| `sensors.rs` | FFI to the LibreHardwareMonitor bridge DLL |
| `ping.rs` | Background ICMP prober (RTT, jitter, loss) |
| `procdetail.rs` | On-demand single-process details |
| `recorder.rs` | Session recording to SQLite, retention, export commands |
| `report.rs` | HTML / CSV / JSON / Markdown report generation |
| `wmi_hub.rs` | WMI connection — used **only** for one-off static queries |
| `etw_util.rs` | ETW session naming (PID-suffixed) and orphan cleanup |

### Design rules worth knowing

**PDH for anything sampled per tick.** WMI's `Win32_PerfFormattedData_*` classes
each block for an internal sampling window; six of them per tick cost 1.7 s.
PDH registers all counters against one query handle and `PdhCollectQueryData`
refreshes them in ~1.4 ms. WMI is kept only for static facts (base clock, DIMM
layout, adapter names) queried once at startup.

**Graceful degradation is mandatory.** Every collector must return "unavailable"
rather than fail the snapshot: no NVIDIA card falls back to PDH counters, no
admin rights disables FPS and temperatures, SMART blocked by Intel RST shows
N/A. A monitor that refuses to start because one sensor is missing is useless.

**The sampling loop is panic-guarded.** `catch_unwind` restarts collection after
3 s and notifies the frontend, so one bad reading cannot silently kill monitoring.

**Elevation happens before Tauri starts.** The manifest declares `asInvoker` and
`elevate.rs` relaunches via `ShellExecute("runas")`. Declaring
`requireAdministrator` instead would break the MSI's "launch app" checkbox, which
uses `CreateProcess` under a non-elevated token and fails silently.

**Expensive collectors run on their own cadence, not on every tick.** The
sampling loop ticks at the user's interval, but three collectors are gated behind
their own timers because they cost far more than everything else combined:

| Collector | Cadence | Cost | Why it can be stale |
|---|---|---|---|
| LibreHardwareMonitor main read | 2 s | ~390 ms | Temperature and package power change on a slower physical timescale |
| SMART (`sysscope_storage_json`) | 10 s | ~70 ms | Drive temperature moves in seconds, health in months, TBW in days |
| Motherboard SuperIO (`sysscope_board_json`) | 5 s | ~1.2 ms | Fan RPM and VRM temperature change slowly |

The LHM figure is the one that matters. LibreHardwareMonitor reads per-core MSRs
by setting thread affinity to each core in turn; on a 20-core/28-thread part that
is ~310 ms to refresh 119 CPU sensors, plus ~77 ms for the NVIDIA domain. **The
cost scales with core count**, so it is worst on exactly the machines people buy
for monitoring-worthy workloads. Reading it every tick put the app at ~40% of one
core; the 2-second cadence halves that, at the price of temperature updating
every 2 s instead of every 1 s.

**A measurement tool that lies about itself is worse than no measurement.**
`README` claimed "~20 ms per tick" for months. That number came from
`perf_breakdown`, which never loaded the sensor bridge in release builds because
`locate_dll`'s source-tree fallback was gated behind `debug_assertions` — so the
most expensive collector was silently absent from every measurement. The claim
survived because the true cost, ~1.4% of a 28-thread machine, is invisible in
Task Manager. Two follow-on lessons are baked into the test now: it measures
`take_snapshot` itself rather than summing collectors called in isolation (the
sum stopped matching reality once cadence gates existed), and the sensor dump
reports min/median/max instead of a single worst case.

**Static hardware facts are collected once, never per tick.** `hwinfo.rs` queries
about a dozen WMI classes plus NVML and IpHelper — roughly 320 ms on a real
machine, dominated by `Nvml::init()`. That runs on the first request only, on a
blocking thread, cached in a `OnceLock` for the process lifetime. It is
deliberately *not* part of the sampler's startup: that budget is 221 ms and
doubling it to show data nobody has asked for yet would be a bad trade.

Note this is a different cost class from the `Win32_PerfFormattedData_*` counters
that used to dominate a tick — those block on an internal sampling window every
time; these just read the CIM repository once.

**Some WMI properties lie; prefer a source that cannot.** Three examples worth
remembering, all found by running the collector on real hardware rather than by
reading docs:

- `Win32_VideoController.AdapterRAM` is a `u32`, so it wraps above 4 GB. VRAM
  comes from NVML instead.
- `Win32_Processor.VirtualizationFirmwareEnabled` reads `False` when Windows
  itself runs on top of Hyper-V, because the hypervisor masks the CPU bits —
  it would report "disabled" on a machine where virtualisation is plainly on.
  `Win32_ComputerSystem.HypervisorPresent` is used as the authority.
- `Win32_NetworkAdapter.PhysicalAdapter` returns `True` for plenty of virtual
  adapters. Physical hardware is identified by a `PCI\` or `USB\` prefix on
  `PNPDeviceID`; without that filter this machine listed eight adapters, five of
  them Wi-Fi Direct, VPN tunnels and TAP devices — one of which advertised a
  fictional 100 Gbps link.

OEMs also fill unset DMI fields with placeholders (`System Product Name`,
`To Be Filled By O.E.M.`, all-zero serials). Those are filtered to "unavailable"
rather than displayed, or they read as real model numbers.

**Language lives in the frontend; the backend keeps a small table.** Almost every
user-visible string is inside the WebView, so `src/i18n.ts` owns the language and
persists the choice. Only the tray menu, the elevation dialog and report output
are native, and `i18n.rs` covers those with a static match — no i18n crate.
The tray is re-labelled in place via `MenuItem::set_text` when the frontend calls
`set_language`, so the UI and the tray never disagree.

One case cannot be fixed: the elevation-failure dialog in `elevate.rs` runs
*before* Tauri starts, when there is no WebView and no way to read the user's
choice, so it always follows the OS display language.

**Hardware labels travel frontend → backend, but masking stays in the backend.**
The 83 hardware labels exist only in the frontend language packs; keeping a
second copy in `i18n.rs` would mean maintaining the same strings in two places.
So the frontend passes already-translated `label` + `value` pairs with the export
request and the report only lays them out.

Masking was deliberately *not* delegated along with them. The frontend sends raw
values plus a `sensitive` flag, and `report.rs::apply_masking` decides whether
full serials reach the file. That makes "nothing identifying is written unless
explicitly enabled" a backend invariant rather than something every call site has
to remember — including call sites that do not exist yet.

**Register state before doing slow work in `setup()`.** Tauri creates the window
and starts loading the frontend *before* `setup()` runs. If `app.manage()` comes
after a slow DB migration, early `invoke` calls fail with "state not managed" and
the frontend's init chain breaks — the panel then shows no data at all.

## Frontend (`src/`)

No framework — plain TypeScript modules, Vite, uPlot for charts.

| File | Responsibility |
|---|---|
| `main.ts` | Bootstrap; subscribes to `metrics` and dispatches to cards |
| `i18n.ts` | `t()`, `applyStatic()`, `setLang()`; packs in `locales/` |
| `types.ts` | Mirrors the backend `Snapshot` shape |
| `charts.ts` | uPlot factory + `RingBuffers` (shared time-series buffer) |
| `tabs.ts` | Tab wiring driven by `data-pane` attributes |
| `format.ts` | Byte/rate/duration formatting, safe DOM helpers |
| `thresholds.ts` | User thresholds (localStorage) + fixed alert constants |
| `cards/*.ts` | One module per card: `build()` DOM once, `update()` per tick |
| `modals/*.ts` | Sessions/export, settings, process detail, hardware inventory |
| `overlay.ts` | The FPS overlay window (separate entry point) |

Three conventions matter:

- **Hidden panes skip rendering.** Each card checks `activePane()` and only
  touches the DOM for the tab currently visible.
- **Untrusted strings go through `textContent`, never `innerHTML`.** Process and
  GPU names come from outside the app.
- **Static markup carries `data-i18n`; dynamic text calls `t()`.** Cards built
  from an `innerHTML` template keep `data-i18n` in the template and call
  `applyStatic(node)` after insertion, so there is only one translation path.

Two checks guard the translations, and they cover different halves. `en.ts` is
declared `Record<Keys, string>` against the Chinese pack, so a missing English
string is a *compile* error. But `data-i18n` values and `t("…")` arguments are
plain strings the type system cannot see — `scripts/check-i18n.mjs` reconciles
those against the packs and also reports keys nothing references. Both run in CI.

## Sensor bridge (`sensor-bridge/`)

LibreHardwareMonitor is a .NET library, so it is compiled with **NativeAOT** into
a plain native DLL exporting a C ABI (`sysscope_sensors_init`,
`sysscope_sensors_json`, `sysscope_sensors_shutdown`). Rust loads it with
`libloading`. The target machine needs no .NET runtime.

`rd.xml` is required: LHM marshals structs through a generic `DeviceIOControl<T>`
that AOT's static analysis cannot see, so the runtime directives force the
marshalling metadata to be generated. Without it, `init` throws at runtime.

The prebuilt DLL is committed at `src-tauri/resources/sysscope_sensors.dll` so
contributors do not need the .NET toolchain.

## Data locations

| What | Where |
|---|---|
| Session database | `%APPDATA%\com.luhaishan.sysscope\sysscope.db` |
| Exported reports | `Documents\SysScope\reports` |
| Thresholds, ping target, language, serial policy | WebView localStorage |

Reports deliberately live in Documents: `%APPDATA%` may be EFS-encrypted, which
makes exported files unopenable from Explorer even though writing them succeeds.

## Testing

- `cargo test` — 50 pure-logic tests (parsing, statistics, schema, escaping, i18n,
  serial masking)
- `cargo test -- --include-ignored` — plus 11 that need real hardware
- `cargo test --lib hwinfo_dump -- --ignored --nocapture` — prints every hardware
  field this machine reports, with timings. Start here when a field shows N/A on
  someone else's box: it separates "WMI returned nothing" from "we dropped it"
- `scripts/smoke-test.ps1` — post-build check that the app actually works:
  process survives, WebView2 children exist, UI thread responds, panel renders
  content, sampler is active
