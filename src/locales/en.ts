/**
 * English pack. `Record<Keys, string>` makes a missing key a compile error,
 * so `tsc --noEmit` in CI catches untranslated strings before they ship.
 */
import type { Keys } from "./zh-CN";

const en: Record<Keys, string> = {
  // Settings dialog
  "settings.title": "Settings",
  "settings.close": "Close",
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
