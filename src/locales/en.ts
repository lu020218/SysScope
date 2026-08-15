/**
 * English pack. `Record<Keys, string>` makes a missing key a compile error,
 * so `tsc --noEmit` in CI catches untranslated strings before they ship.
 *
 * Keep labels short: English runs 1.5-2x longer than the Chinese they replace,
 * and the cards were sized around the Chinese text.
 */
import type { Keys } from "./zh-CN";

const en: Record<Keys, string> = {
  // Common
  "common.close": "Close",

  // Title bar and toolbar
  "app.loading": "Loading system info…",
  "app.crashWarn":
    "The sampling thread crashed and restarted — data may have a short gap",
  "win.onTop": "Always on top",
  "win.settings": "Settings",
  "win.min": "Minimise",
  "win.max": "Maximise / restore",
  "win.close": "Close to tray",
  "toolbar.record.title": "Start / stop recording a session",
  "toolbar.sessions": "Reports",
  "toolbar.sessions.title": "Browse sessions and export reports",
  "toolbar.overlay": "Overlay",
  "toolbar.overlay.title": "Show / hide the FPS overlay",
  "toolbar.interval.title": "Sampling interval",

  // Card titles and shared tabs
  "card.mem": "Memory",
  "card.net": "Network",
  "card.disk": "Disk",
  "card.procs": "Top 5 processes",
  "tab.overview": "Overview",
  "tab.detail": "Details",

  // CPU
  "cpu.stat.total": "Total",
  "cpu.stat.temp": "Temp",
  "cpu.stat.power": "Power",
  "cpu.stat.freq": "Clock",
  "cpu.tab.cores": "Cores",
  "cpu.tab.freq": "Clocks",
  "cpu.tab.power": "Power",
  "cpu.freq.cur": "Current",
  "cpu.freq.eff": "Effective",
  "cpu.freq.base": "Base",
  "cpu.freq.max": "Session peak",
  "cpu.freq.avg": "Average",
  "cpu.freq.boost": "Boosting",
  "cpu.power.package": "Package power",
  "cpu.power.peak": "Session peak power",
  "cpu.power.voltage": "Core voltage",
  "cpu.power.boost": "Boost state",
  "cpu.power.c1": "C1 residency",
  "cpu.power.c2": "C2 residency",
  "cpu.power.c3": "C3 residency",

  // Memory
  "mem.stat.pct": "Usage",
  "mem.stat.used": "Used / total",
  "mem.sub.avail": "Available",
  "mem.sub.commit": "Committed",
  "mem.sub.faults": "Hard faults",
  "mem.detail.cached": "Cached",
  "mem.detail.standby": "Standby / modified",
  "mem.detail.pf": "Page faults (all / hard)",
  "mem.detail.compression": "Compressed",
  "mem.detail.swap": "Page file",
  "mem.detail.commitPct": "Commit ratio",
  "mem.detail.speed": "DIMM speed",
  "mem.detail.bandwidth": "Theoretical bandwidth",

  // Network
  "net.stat.down": "↓ Down",
  "net.stat.up": "↑ Up",
  "net.legend.down": "Down",
  "net.legend.up": "Up",
  "net.sub.iface": "Adapter",
  "net.sub.ping": "Latency",
  "net.sub.loss": "Loss",

  // Disk
  "disk.stat.active": "Active",
  "disk.read": "Read",
  "disk.write": "Write",

  // Top-5 column headers — the columns are narrow, keep these to one short word
  "procs.col.mem": "Memory",
  "procs.col.net": "Network",
  "procs.col.disk": "Disk",
  "procs.col.gpu": "GPU · VRAM",

  // Process detail dialog
  "pd.title": "Process details",
  "pd.cputime": "Total CPU time",
  "pd.threads": "Threads",
  "pd.handles": "Handles",
  "pd.ws": "Working set / peak",
  "pd.priv": "Private bytes",
  "pd.pf": "Page faults",
  "pd.prio": "Priority",
  "pd.affinity": "CPU affinity",

  // Sessions dialog
  "sessions.title": "Recorded sessions",
  "sessions.openDir": "📁 Reports folder",
  "sessions.openDir.title": "Open the reports folder",

  // Settings dialog
  "settings.title": "Settings",
  "settings.language": "Language",
  "settings.thresholds": "Alert thresholds",
  "settings.th.cpu": "CPU usage (%)",
  "settings.th.mem": "Memory usage (%)",
  "settings.th.gpu": "GPU usage (%)",
  "settings.th.cpuTemp": "CPU temperature (°C)",
  "settings.th.gpuTemp": "GPU temperature (°C)",
  "settings.probe": "Network probe",
  "settings.probe.target": "Latency probe target (IP / hostname)",
  "settings.diag": "Diagnostics",
  "settings.diag.sampleCost": "Sampling cost per tick",
  "settings.reset": "Restore defaults",

  // Overlay strip — keep these abbreviated, the window sizes to its content
  "osd.mem": "MEM",
  "osd.net": "NET",
};

export default en;
