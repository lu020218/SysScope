# Architecture

A map of the codebase for anyone who wants to change it.

## Shape of the app

```
                    ┌──────────────────────────────────────┐
                    │  sampling thread (one, owns all       │
                    │  collectors; panic-guarded)           │
                    │                                       │
   PDH ────────────▶│  take_snapshot() every tick (~20 ms)  │
   NVML ───────────▶│         │                             │
   ETW ────────────▶│         ├──▶ SQLite (when recording)   │
   LHM bridge ─────▶│         │                             │
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

**Language lives in the frontend; the backend keeps a small table.** Almost every
user-visible string is inside the WebView, so `src/i18n.ts` owns the language and
persists the choice. Only the tray menu, the elevation dialog and report output
are native, and `i18n.rs` covers those with a static match — no i18n crate.
The tray is re-labelled in place via `MenuItem::set_text` when the frontend calls
`set_language`, so the UI and the tray never disagree.

One case cannot be fixed: the elevation-failure dialog in `elevate.rs` runs
*before* Tauri starts, when there is no WebView and no way to read the user's
choice, so it always follows the OS display language.

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
| `modals/*.ts` | Sessions/export, settings, process detail |
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
| Thresholds, ping target | WebView localStorage |

Reports deliberately live in Documents: `%APPDATA%` may be EFS-encrypted, which
makes exported files unopenable from Explorer even though writing them succeeds.

## Testing

- `cargo test` — 29 pure-logic tests (parsing, statistics, schema, escaping, i18n)
- `cargo test -- --include-ignored` — plus 8 that need real hardware
- `scripts/smoke-test.ps1` — post-build check that the app actually works:
  process survives, WebView2 children exist, UI thread responds, panel renders
  content, sampler is active
