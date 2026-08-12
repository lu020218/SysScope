import type uPlot from "uplot";
import { buffers, makeChart } from "../charts";
import { $, addInfoBlock, fmtBytes, fmtRate, setWarn } from "../format";
import { activePane } from "../tabs";
import type { Snapshot } from "../types";

let chart: uPlot;
let card: HTMLElement;

export function init() {
  card = $("disk-card");
  chart = makeChart(
    $("disk-chart"),
    [
      { color: "#2dd4bf", fill: "rgba(45,212,191,0.12)" },
      { color: "#fbbf24", dash: [4, 4] },
    ],
    { floor: 10 * 1024 * 1024, fmt: fmtRate },
  );
}

export function update(s: Snapshot, ts: number[], start: number) {
  const diskActive = s.storage.disks.reduce((m, d) => Math.max(m, d.active_pct), 0);
  const activeEl = $("disk-active");
  activeEl.textContent = `${diskActive.toFixed(0)}%`;
  setWarn(activeEl, diskActive >= 90);
  const read = s.storage.disks.reduce((a, d) => a + d.read_bps, 0);
  const write = s.storage.disks.reduce((a, d) => a + d.write_bps, 0);
  $("disk-read").textContent = fmtRate(read);
  $("disk-write").textContent = fmtRate(write);

  const pane = activePane(card);

  if (pane === "ov") {
    $("disk-vols").textContent = s.storage.volumes
      .map((v) => `${v.mount.replace(/\\$/, "")} 可用${fmtBytes(v.available)}`)
      .join(" · ");
    $("disk-temps").textContent = s.storage_temps
      .filter((t) => t.temp != null)
      .map((t) => `${t.name.split(" ").slice(-1)[0]} ${t.temp!.toFixed(0)}°`)
      .join(" · ");
    chart.setData([
      ts,
      buffers.series("dread").slice(start),
      buffers.series("dwrite").slice(start),
    ]);
  }

  if (pane === "detail") {
    renderDetail(s);
  }
}

/** 磁盘详情页：每物理盘的 IOPS/延迟/队列 + SMART 信息块 */
function renderDetail(s: Snapshot) {
  const wrap = $("disk-detail");
  wrap.innerHTML = "";
  const ms = (v: number | null) => (v != null ? `${v.toFixed(2)} ms` : "--");
  for (const d of s.storage.disks) {
    addInfoBlock(wrap, `磁盘 ${d.name}`, [
      ["读 IOPS", d.read_iops.toFixed(0)],
      ["写 IOPS", d.write_iops.toFixed(0)],
      ["读延迟", ms(d.read_ms)],
      ["写延迟", ms(d.write_ms)],
      ["队列", d.queue_len.toFixed(0)],
    ]);
  }
  for (const t of s.storage_temps) {
    addInfoBlock(wrap, t.name, [
      ["温度", t.temp != null ? `${t.temp.toFixed(0)}°C` : "N/A"],
      ["控制器", t.temp2 != null ? `${t.temp2.toFixed(0)}°C` : "N/A"],
      ["健康", t.life != null ? `${t.life.toFixed(0)}%` : "N/A"],
      [
        "累计写入",
        t.written_gb != null ? `${(t.written_gb / 1024).toFixed(2)} TB` : "N/A",
      ],
    ]);
  }
  if (s.storage_temps.length === 0) {
    const note = document.createElement("div");
    note.className = "sessions-empty";
    note.textContent = "SMART 信息不可用（可能被 Intel RST/VMD 拦截）";
    wrap.appendChild(note);
  }
}
