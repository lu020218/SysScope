import type uPlot from "uplot";
import { buffers, makeChart } from "../charts";
import { $, fmtBytes, setWarn } from "../format";
import { activePane } from "../tabs";
import type { Thresholds } from "../thresholds";
import type { Snapshot } from "../types";

let chart: uPlot;
let card: HTMLElement;

export function init() {
  card = $("mem-card");
  chart = makeChart($("mem-chart"), [
    { color: "#34d399", fill: "rgba(52,211,153,0.12)" },
  ]);
}

export function update(s: Snapshot, th: Thresholds, ts: number[], start: number) {
  const pct = (s.mem.used / s.mem.total) * 100;
  const memEl = $("mem-pct");
  memEl.textContent = `${pct.toFixed(1)}%`;
  setWarn(memEl, pct >= th.mem);
  $("mem-used").textContent = `${fmtBytes(s.mem.used)} / ${fmtBytes(s.mem.total)}`;

  const pane = activePane(card);

  if (pane === "ov") {
    $("mem-avail").textContent = fmtBytes(s.mem.available);
    $("mem-commit").textContent =
      s.mem.commit_limit > 0
        ? `${fmtBytes(s.mem.commit_used)} / ${fmtBytes(s.mem.commit_limit)}`
        : "N/A";
    const faultsEl = $("mem-faults");
    faultsEl.textContent = `${s.mem.hard_faults_ps.toFixed(0)}/s`;
    setWarn(faultsEl, s.mem.hard_faults_ps > 200);
    chart.setData([ts, buffers.series("mem").slice(start)]);
  }

  if (pane === "detail") {
    $("mm-cached").textContent = fmtBytes(s.mem.standby_bytes + s.mem.modified_bytes);
    $("mm-standby").textContent =
      `${fmtBytes(s.mem.standby_bytes)} / ${fmtBytes(s.mem.modified_bytes)}`;
    $("mm-pf").textContent =
      `${s.mem.page_faults_ps.toFixed(0)} / ${s.mem.hard_faults_ps.toFixed(0)} 每秒`;
    $("mm-comp").textContent =
      s.mem.compression != null ? fmtBytes(s.mem.compression) : "N/A";
    $("mm-swap").textContent =
      `${fmtBytes(s.mem.swap_used)} / ${fmtBytes(s.mem.swap_total)}`;
    const commitPctEl = $("mm-commitpct");
    if (s.mem.commit_limit > 0) {
      const cp = (s.mem.commit_used / s.mem.commit_limit) * 100;
      commitPctEl.textContent = `${cp.toFixed(1)}%`;
      setWarn(commitPctEl, cp >= 90);
    } else {
      commitPctEl.textContent = "N/A";
    }
    $("mm-speed").textContent =
      s.mem.mem_speed_mts > 0
        ? `${s.mem.mem_speed_mts} MT/s × ${s.mem.mem_modules} 条`
        : "N/A";
    $("mm-bw").textContent =
      s.mem.theo_bandwidth_gbps > 0
        ? `≈ ${s.mem.theo_bandwidth_gbps.toFixed(1)} GB/s`
        : "N/A";
  }
}
