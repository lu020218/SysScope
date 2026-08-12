import type uPlot from "uplot";
import { buffers, makeChart } from "../charts";
import { $, setWarn } from "../format";
import { activePane } from "../tabs";
import type { Thresholds } from "../thresholds";
import type { Snapshot, StaticInfo } from "../types";

let chart: uPlot;
let card: HTMLElement;

/** 每逻辑核心效率等级（P/E 分组与亲和性位图共用） */
let coreClasses: number[] = [];
export function getCoreClasses(): number[] {
  return coreClasses;
}

/** 热节流判定与频率会话统计 */
let maxFreqSeen = 0;
let fqMax = 0;
let fqMin = Infinity;
let fqSum = 0;
let fqN = 0;

// ---------- 每核心竖状迷你条（核心分组页） ----------

function makeVcore(i: number): [HTMLElement, HTMLElement] {
  const bar = document.createElement("div");
  bar.className = "vcore";
  bar.title = `C${i}`;
  const fill = document.createElement("div");
  fill.className = "vcore-fill";
  bar.appendChild(fill);
  return [bar, fill];
}

interface PeGroup {
  avgEl: HTMLElement;
  indices: number[];
}

let peFills: HTMLElement[] = [];
let peBars: HTMLElement[] = [];
let peGroups: PeGroup[] = [];

function buildPeGroups(n: number) {
  const wrap = $("pe-groups");
  wrap.innerHTML = "";
  peFills = new Array(n);
  peBars = new Array(n);
  peGroups = [];
  const classes = coreClasses.length === n ? coreClasses : new Array(n).fill(0);
  const uniq = [...new Set(classes)].sort((a, b) => b - a);
  const maxClass = uniq[0];
  const hybrid = uniq.length > 1;
  for (const cls of uniq) {
    const indices = classes
      .map((c, i) => [c, i] as [number, number])
      .filter(([c]) => c === cls)
      .map(([, i]) => i);
    const group = document.createElement("div");
    group.className = "pe-group";
    const head = document.createElement("div");
    head.className = "pe-head";
    const title = document.createElement("span");
    title.className = "pe-title";
    title.textContent = hybrid
      ? `${cls === maxClass ? "P-Core" : "E-Core"} × ${indices.length} 线程`
      : `全部核心 × ${indices.length}`;
    const avg = document.createElement("b");
    avg.className = "pe-avg";
    avg.textContent = "--%";
    head.append(title, avg);
    const strip = document.createElement("div");
    strip.className = "cores";
    for (const i of indices) {
      const [bar, fill] = makeVcore(i);
      strip.appendChild(bar);
      peBars[i] = bar;
      peFills[i] = fill;
    }
    group.append(head, strip);
    wrap.appendChild(group);
    peGroups.push({ avgEl: avg, indices });
  }
}

// ---------- 频率页时钟格 ----------

let clockEls: HTMLElement[] = [];

function buildClockGrid(n: number) {
  const wrap = $("clock-grid");
  wrap.innerHTML = "";
  clockEls = [];
  for (let i = 0; i < n; i++) {
    const chip = document.createElement("div");
    chip.className = "clock-chip";
    const label = document.createElement("span");
    label.textContent = `C${i}`;
    const val = document.createElement("b");
    val.textContent = "--";
    chip.append(label, val);
    wrap.appendChild(chip);
    clockEls.push(val);
  }
}

// ---------- 生命周期 ----------

export function init(info: StaticInfo | null) {
  card = $("cpu-card");
  chart = makeChart($("cpu-chart"), [
    { color: "#38bdf8", fill: "rgba(56,189,248,0.12)" },
  ]);
  if (info) {
    coreClasses = info.core_classes;
    buildPeGroups(info.logical_cores);
  }
}

export function update(s: Snapshot, th: Thresholds, ts: number[], start: number) {
  // 头部数值（任何面板下都更新）
  const cpuEl = $("cpu-total");
  cpuEl.textContent = `${s.cpu.total.toFixed(1)}%`;
  setWarn(cpuEl, s.cpu.total >= th.cpu);
  const tempEl = $("cpu-temp");
  tempEl.textContent = s.cpu.temp_c != null ? `${s.cpu.temp_c.toFixed(0)}°C` : "N/A";
  setWarn(tempEl, s.cpu.temp_c != null && s.cpu.temp_c >= th.cpuTemp);
  $("cpu-power").textContent =
    s.cpu.power_w != null ? `${s.cpu.power_w.toFixed(0)} W` : "N/A";
  // 热节流判定：温度逼近阈值且频率明显低于本次会话观测到的峰值
  maxFreqSeen = Math.max(maxFreqSeen, s.cpu.freq_mhz);
  const throttling =
    s.cpu.temp_c != null &&
    s.cpu.temp_c >= th.cpuTemp - 5 &&
    s.cpu.freq_mhz < maxFreqSeen * 0.85;
  const freqEl = $("cpu-freq");
  freqEl.textContent = `${s.cpu.freq_mhz} MHz${throttling ? " ⚠节流" : ""}`;
  setWarn(freqEl, throttling);

  // 会话统计始终累计（切到频率页时数据完整）
  fqMax = Math.max(fqMax, s.cpu.freq_mhz);
  if (s.cpu.freq_mhz > 0) {
    fqMin = Math.min(fqMin, s.cpu.freq_mhz);
    fqSum += s.cpu.freq_mhz;
    fqN += 1;
  }

  const pane = activePane(card);

  if (pane === "overview") {
    chart.setData([ts, buffers.series("cpu").slice(start)]);
  }

  if (pane === "cores") {
    if (s.cpu.per_core.length !== peFills.length) {
      buildPeGroups(s.cpu.per_core.length);
    }
    s.cpu.per_core.forEach((v, i) => {
      if (peFills[i]) {
        peFills[i].style.height = `${Math.max(4, v).toFixed(0)}%`;
        peFills[i].classList.toggle("warn", v >= th.cpu);
        peBars[i].title = `C${i}  ${v.toFixed(0)}%`;
      }
    });
    for (const g of peGroups) {
      const avg =
        g.indices.reduce((a, i) => a + (s.cpu.per_core[i] ?? 0), 0) /
        Math.max(1, g.indices.length);
      g.avgEl.textContent = `${avg.toFixed(0)}%`;
    }
  }

  if (pane === "freq") {
    $("fq-cur").textContent = `${s.cpu.freq_mhz} MHz`;
    $("fq-eff").textContent =
      s.cpu.effective_mhz != null
        ? `${(s.cpu.effective_mhz / 1000).toFixed(2)} GHz`
        : "N/A";
    $("fq-base").textContent = s.cpu.base_mhz > 0 ? `${s.cpu.base_mhz} MHz` : "N/A";
    $("fq-max").textContent = `${fqMax} MHz`;
    $("fq-avg").textContent = fqN > 0 ? `${Math.round(fqSum / fqN)} MHz` : "--";
    $("fq-boost").classList.toggle("hidden", !s.cpu.boost);
    if (s.cpu.core_clocks.length !== clockEls.length) {
      buildClockGrid(s.cpu.core_clocks.length);
    }
    s.cpu.core_clocks.forEach((mhz, i) => {
      clockEls[i].textContent = `${(mhz / 1000).toFixed(2)}G`;
    });
  }

  if (pane === "power") {
    $("pw-cur").textContent =
      s.cpu.power_w != null ? `${s.cpu.power_w.toFixed(1)} W` : "N/A";
    $("pw-peak").textContent =
      s.cpu.power_peak_w != null ? `${s.cpu.power_peak_w.toFixed(1)} W` : "N/A";
    $("pw-volt").textContent =
      s.cpu.voltage_v != null ? `${s.cpu.voltage_v.toFixed(3)} V` : "N/A";
    $("pw-boost").textContent = s.cpu.boost ? "睿频中" : "基准运行";
    const cs = s.cpu.perf;
    for (const [id, v] of [
      ["cs1", cs?.c1_pct],
      ["cs2", cs?.c2_pct],
      ["cs3", cs?.c3_pct],
    ] as const) {
      $(id).style.width = `${Math.min(100, v ?? 0)}%`;
      $(`${id}v`).textContent = v != null ? `${v.toFixed(0)}%` : "N/A";
    }
  }
}
