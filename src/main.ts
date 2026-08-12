import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import "./styles.css";
import { DEFAULTS, saveThresholds, thresholds } from "./thresholds";

interface CpuPerf {
  perf_pct: number;
  c1_pct: number;
  c2_pct: number;
  c3_pct: number;
}

interface CpuSnapshot {
  total: number;
  per_core: number[];
  freq_mhz: number;
  temp_c: number | null;
  power_w: number | null;
  power_peak_w: number | null;
  voltage_v: number | null;
  core_clocks: number[];
  base_mhz: number;
  effective_mhz: number | null;
  boost: boolean;
  perf: CpuPerf | null;
}

interface MemSnapshot {
  total: number;
  used: number;
  available: number;
  swap_total: number;
  swap_used: number;
  compression: number | null;
  commit_used: number;
  commit_limit: number;
  hard_faults_ps: number;
  page_faults_ps: number;
  standby_bytes: number;
  modified_bytes: number;
  mem_speed_mts: number;
  mem_modules: number;
  theo_bandwidth_gbps: number;
}

interface GpuSnapshot {
  name: string;
  util_pct: number;
  vram_used: number;
  vram_total: number;
  temp_c: number | null;
  power_w: number | null;
  power_limit_w: number | null;
  core_mhz: number | null;
  mem_mhz: number | null;
  fan_pct: number | null;
  mem_ctrl_pct: number | null;
  enc_pct: number | null;
  dec_pct: number | null;
  pcie_rx_kbs: number | null;
  pcie_tx_kbs: number | null;
  throttle_thermal: boolean;
  throttle_power: boolean;
  temp_slowdown_c: number | null;
  hotspot_c: number | null;
  fan_rpm: number | null;
  vram_temp_c: number | null;
}

interface FpsSnapshot {
  status: "ok" | "no_admin" | "failed";
  pid: number;
  process: string;
  fps: number;
  frame_time_ms: number;
  low_1pct_fps: number;
  has_data: boolean;
}

interface NetIface {
  name: string;
  down_bps: number;
  up_bps: number;
  total_rx: number;
  total_tx: number;
}

interface PingStats {
  target: string;
  rtt_ms: number | null;
  avg_ms: number;
  jitter_ms: number;
  loss_pct: number;
  active: boolean;
}

interface AdapterUtil {
  name: string;
  link_bps: number;
  util_pct: number;
}

interface NetSnapshot {
  down_bps: number;
  up_bps: number;
  total_rx: number;
  total_tx: number;
  ifaces: NetIface[];
  ping: PingStats;
  tcp_established: number;
  tcp_time_wait: number;
  tcp_listen: number;
  udp_endpoints: number;
  retrans_ps: number;
  retrans_pct: number;
  adapters: AdapterUtil[];
}

interface ProcNetStat {
  pid: number;
  name: string;
  down_bps: number;
  up_bps: number;
}

interface DiskIo {
  name: string;
  active_pct: number;
  read_bps: number;
  write_bps: number;
  queue_len: number;
  read_iops: number;
  write_iops: number;
  read_ms: number | null;
  write_ms: number | null;
}

interface VolumeInfo {
  mount: string;
  total: number;
  available: number;
}

interface StorageSnapshot {
  disks: DiskIo[];
  volumes: VolumeInfo[];
}

interface StorageTemp {
  name: string;
  temp: number | null;
  temp2: number | null;
  life: number | null;
  written_gb: number | null;
}

interface ProcStat {
  pid: number;
  name: string;
  cpu_pct: number;
  mem: number;
  disk_bps: number;
}

interface GpuProcStat {
  pid: number;
  name: string;
  gpu_pct: number;
  vram: number;
}

interface ProcDetail {
  pid: number;
  cpu_time_ms: number;
  threads: number;
  handles: number;
  working_set: number;
  working_set_peak: number;
  private_bytes: number;
  page_faults: number;
  priority: string;
  affinity_mask: number;
  ok: boolean;
}

interface Snapshot {
  ts: number;
  cpu: CpuSnapshot;
  mem: MemSnapshot;
  gpus: GpuSnapshot[];
  fps: FpsSnapshot;
  net: NetSnapshot;
  storage: StorageSnapshot;
  storage_temps: StorageTemp[];
  top_cpu: ProcStat[];
  top_mem: ProcStat[];
  top_net: ProcNetStat[];
  top_disk: ProcStat[];
  top_gpu: GpuProcStat[];
}

interface RecStatus {
  active: boolean;
  session_id: number | null;
  started_at: number | null;
  samples: number;
}

interface SessionMeta {
  id: number;
  started_at: number;
  ended_at: number | null;
  samples: number;
}

interface StaticInfo {
  cpu_name: string;
  logical_cores: number;
  physical_cores: number | null;
  total_mem: number;
  os: string;
  hostname: string;
  core_classes: number[];
}

/** 数据缓冲上限：30min @ 0.5s = 3600 点，留些余量 */
const MAX_POINTS = 4000;

let windowSec = 60;

const timestamps: number[] = [];
const cpuTotal: number[] = [];
const memPct: number[] = [];
const gpuUtil: number[][] = [];
const gpuVramPct: number[][] = [];
const netDown: number[] = [];
const netUp: number[] = [];
const diskRead: number[] = [];
const diskWrite: number[] = [];
/** 会话内观测到的最高频率，用于热节流判定 */
let maxFreqSeen = 0;
/** 频率会话统计 */
let fqMax = 0;
let fqMin = Infinity;
let fqSum = 0;
let fqN = 0;
/** 每逻辑核心效率等级（来自 StaticInfo） */
let coreClasses: number[] = [];

const $ = (id: string) => document.getElementById(id)!;

function fmtBytes(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

function fmtRate(bps: number): string {
  if (bps >= 1024 ** 3) return `${(bps / 1024 ** 3).toFixed(2)} GB/s`;
  if (bps >= 1024 ** 2) return `${(bps / 1024 ** 2).toFixed(1)} MB/s`;
  if (bps >= 1024) return `${(bps / 1024).toFixed(0)} KB/s`;
  return `${bps.toFixed(0)} B/s`;
}

interface SeriesDef {
  color: string;
  fill?: string;
  dash?: number[];
}

interface YCfg {
  /** 固定 0-100 百分比轴 */
  pct?: boolean;
  /** 自动轴的最小上界 */
  floor?: number;
  /** 自定义刻度格式 */
  fmt?: (v: number) => string;
}

function makeChart(el: HTMLElement, series: SeriesDef[], y: YCfg = { pct: true }): uPlot {
  const opts: uPlot.Options = {
    width: el.clientWidth || 400,
    height: el.clientHeight || 150,
    padding: [8, 8, 0, 0],
    cursor: { show: false },
    scales: {
      x: { time: true },
      y: y.pct
        ? { range: [0, 100] }
        : { range: (_u, _min, max) => [0, Math.max(y.floor ?? 10, (max || 0) * 1.15)] },
    },
    axes: [
      {
        stroke: "#69748a",
        grid: { stroke: "#1b222d", width: 1 },
        ticks: { show: false },
        font: "10px Segoe UI",
      },
      {
        stroke: "#69748a",
        grid: { stroke: "#1b222d", width: 1 },
        ticks: { show: false },
        font: "10px Segoe UI",
        size: y.fmt ? 58 : 36,
        values: (_u, vals) =>
          vals.map((v) => (y.fmt ? y.fmt(v) : y.pct ? `${v}%` : `${v}`)),
      },
    ],
    series: [
      {},
      ...series.map((s) => ({
        stroke: s.color,
        fill: s.fill,
        width: 1.5,
        dash: s.dash,
        points: { show: false },
      })),
    ],
  };
  const chart = new uPlot(opts, [[], ...series.map(() => [])] as uPlot.AlignedData, el);
  new ResizeObserver(() => {
    chart.setSize({ width: el.clientWidth, height: el.clientHeight });
  }).observe(el);
  return chart;
}

/** 当前时间窗对应的起始下标 */
function windowStart(): number {
  const cutoff = Date.now() / 1000 - windowSec;
  const i = timestamps.findIndex((t) => t >= cutoff);
  return i < 0 ? timestamps.length : i;
}

function setWarn(el: HTMLElement, warn: boolean) {
  el.classList.toggle("warn", warn);
}

/** 磁盘详情页：每物理盘的 IOPS/延迟/队列 + SMART 信息块 */
function renderDiskDetail(s: Snapshot) {
  const wrap = $("disk-detail");
  wrap.innerHTML = "";
  const addBlock = (name: string, pairs: [string, string][]) =>
    addInfoBlock(wrap, name, pairs);
  const ms = (v: number | null) => (v != null ? `${v.toFixed(2)} ms` : "--");
  for (const d of s.storage.disks) {
    addBlock(`磁盘 ${d.name}`, [
      ["读 IOPS", d.read_iops.toFixed(0)],
      ["写 IOPS", d.write_iops.toFixed(0)],
      ["读延迟", ms(d.read_ms)],
      ["写延迟", ms(d.write_ms)],
      ["队列", d.queue_len.toFixed(0)],
    ]);
  }
  for (const t of s.storage_temps) {
    addBlock(t.name, [
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

/** 通用信息块（磁盘/网络详情页共用） */
function addInfoBlock(wrap: HTMLElement, name: string, pairs: [string, string][]) {
  const block = document.createElement("div");
  block.className = "dd-block";
  const title = document.createElement("div");
  title.className = "dd-name";
  title.textContent = name;
  const row = document.createElement("div");
  row.className = "dd-row";
  for (const [k, v] of pairs) {
    const span = document.createElement("span");
    const b = document.createElement("b");
    b.textContent = v;
    span.append(`${k} `, b);
    row.appendChild(span);
  }
  block.append(title, row);
  wrap.appendChild(block);
}

/** 网络详情页：连接统计 + 重传 + 各网卡链路利用率 */
function renderNetDetail(s: Snapshot) {
  const wrap = $("net-detail");
  wrap.innerHTML = "";
  addInfoBlock(wrap, "TCP / UDP 连接", [
    ["已建立", String(s.net.tcp_established)],
    ["TIME_WAIT", String(s.net.tcp_time_wait)],
    ["监听", String(s.net.tcp_listen)],
    ["UDP 端点", String(s.net.udp_endpoints)],
  ]);
  addInfoBlock(wrap, "TCP 重传", [
    ["重传", `${s.net.retrans_ps.toFixed(0)}/s`],
    ["重传率", `${s.net.retrans_pct.toFixed(2)}%`],
  ]);
  for (const a of s.net.adapters) {
    addInfoBlock(wrap, a.name, [
      ["链路", fmtLinkSpeed(a.link_bps)],
      ["利用率", `${a.util_pct.toFixed(1)}%`],
    ]);
  }
}

function fmtLinkSpeed(bps: number): string {
  if (bps >= 1e9) return `${(bps / 1e9).toFixed(bps % 1e9 === 0 ? 0 : 1)} Gbps`;
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(0)} Mbps`;
  return `${(bps / 1e3).toFixed(0)} Kbps`;
}

/** 用 DOM API 渲染进程列表（进程名不可信，避免 innerHTML 注入）；点击行打开进程详情 */
function renderProcs<T extends { pid: number; name: string }>(
  el: HTMLElement,
  list: T[],
  fmt: (p: T) => string,
) {
  el.innerHTML = "";
  for (const p of list) {
    const row = document.createElement("div");
    row.className = "proc-row";
    const name = document.createElement("span");
    name.className = "proc-name";
    name.textContent = p.name;
    name.title = `PID ${p.pid} · 点击查看详情`;
    const val = document.createElement("span");
    val.className = "proc-val";
    val.textContent = fmt(p);
    row.append(name, val);
    row.addEventListener("click", () => openProcDetail(p.pid, p.name));
    el.appendChild(row);
  }
}

// ---------- 进程详情弹窗 ----------

let pdTimer: number | null = null;

function fmtCpuTime(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

function renderAffinity(mask: number) {
  const wrap = $("pd-affinity");
  wrap.innerHTML = "";
  const n = Math.max(coreClasses.length, 1);
  for (let i = 0; i < n; i++) {
    const dot = document.createElement("span");
    dot.className = "aff-dot";
    // 53 位内安全；掩码高于核心数的位不展示
    if (i < 53 && (mask / 2 ** i) % 2 >= 1) dot.classList.add("on");
    dot.title = `C${i}`;
    wrap.appendChild(dot);
  }
}

function closeProcDetail() {
  if (pdTimer != null) {
    clearInterval(pdTimer);
    pdTimer = null;
  }
  $("proc-modal").classList.add("hidden");
}

function openProcDetail(pid: number, name: string) {
  closeProcDetail();
  $("pd-title").textContent = `${name} · PID ${pid}`;
  $("proc-modal").classList.remove("hidden");
  const tick = async () => {
    const d = await invoke<ProcDetail>("process_detail", { pid });
    if (!d.ok) {
      $("pd-prio").textContent = "进程已退出或无法访问";
      return;
    }
    $("pd-cputime").textContent = fmtCpuTime(d.cpu_time_ms);
    $("pd-threads").textContent = String(d.threads);
    $("pd-handles").textContent = String(d.handles);
    $("pd-ws").textContent =
      `${fmtBytes(d.working_set)} / ${fmtBytes(d.working_set_peak)}`;
    $("pd-priv").textContent = fmtBytes(d.private_bytes);
    $("pd-pf").textContent = d.page_faults.toLocaleString();
    $("pd-prio").textContent = d.priority;
    renderAffinity(d.affinity_mask);
  };
  void tick();
  pdTimer = window.setInterval(tick, 1000);
}

// ---------- 每核心竖状迷你条（核心分组页使用） ----------

function makeVcore(i: number): [HTMLElement, HTMLElement] {
  const bar = document.createElement("div");
  bar.className = "vcore";
  bar.title = `C${i}`;
  const fill = document.createElement("div");
  fill.className = "vcore-fill";
  bar.appendChild(fill);
  return [bar, fill];
}

// ---------- P/E 核心分组页 ----------

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

// ---------- GPU 动态卡片 ----------

interface GpuCard {
  name: string;
  chart: uPlot;
  utilEl: HTMLElement;
  vramEl: HTMLElement;
  tempEl: HTMLElement;
  powerEl: HTMLElement;
  coreEl: HTMLElement;
  fanEl: HTMLElement;
  badgeThermal: HTMLElement;
  badgePower: HTMLElement;
  kv: Record<string, HTMLElement>;
}

let gpuCards: GpuCard[] = [];

function buildGpuCards(gpus: GpuSnapshot[]) {
  document.querySelectorAll(".gpu-card").forEach((el) => el.remove());
  gpuCards = [];
  gpuUtil.length = 0;
  gpuVramPct.length = 0;

  const grid = document.querySelector(".grid")!;
  gpus.forEach((g, i) => {
    gpuUtil.push(new Array(timestamps.length).fill(NaN));
    gpuVramPct.push(new Array(timestamps.length).fill(NaN));

    const card = document.createElement("section");
    card.className = "card gpu-card";
    card.innerHTML = `
      <div class="card-head">
        <h2 class="gpu-title">GPU${gpus.length > 1 ? ` ${i}` : ""} · ${g.name}</h2>
        <div class="head-right">
          <span class="tbadge t-thermal hidden">热节流</span>
          <span class="tbadge t-power hidden">功耗墙</span>
          <div class="stats">
            <div class="stat">
              <span class="stat-value gpu-util">--%</span>
              <span class="stat-label">占用</span>
            </div>
            <div class="stat">
              <span class="stat-value gpu-vram">--</span>
              <span class="stat-label">显存</span>
            </div>
          </div>
        </div>
      </div>
      <div class="tabs gpu-tabs">
        <button data-pane="ov" class="active">概览</button>
        <button data-pane="detail">详情</button>
      </div>
      <div class="tab-pane pane-ov">
        <div class="chart gpu-chart"></div>
        <div class="substats">
          <span class="legend legend-util">占用率</span>
          <span class="legend legend-vram">显存</span>
          <span>温度 <b class="gpu-temp">--</b></span>
          <span>功耗 <b class="gpu-power">--</b></span>
          <span>核心 <b class="gpu-core">--</b></span>
          <span>风扇 <b class="gpu-fan">--</b></span>
        </div>
      </div>
      <div class="tab-pane pane-detail hidden">
        <div class="kv-rows kv-2col">
          <div class="kv"><span>热点温度</span><b data-kv="hotspot">--</b></div>
          <div class="kv"><span>显存温度</span><b data-kv="vramtemp">--</b></div>
          <div class="kv"><span>降频阈值</span><b data-kv="slowdown">--</b></div>
          <div class="kv"><span>风扇</span><b data-kv="fan2">--</b></div>
          <div class="kv"><span>显存控制器负载</span><b data-kv="memctrl">--</b></div>
          <div class="kv"><span>视频编码 / 解码</span><b data-kv="codec">--</b></div>
          <div class="kv"><span>PCIe 接收</span><b data-kv="pcierx">--</b></div>
          <div class="kv"><span>PCIe 发送</span><b data-kv="pcietx">--</b></div>
        </div>
      </div>`;
    grid.insertBefore(card, $("net-card"));

    card.querySelector(".gpu-tabs")!.addEventListener("click", (e) => {
      const btn = (e.target as HTMLElement).closest("button");
      if (!btn) return;
      card
        .querySelectorAll(".gpu-tabs button")
        .forEach((b) => b.classList.toggle("active", b === btn));
      card
        .querySelector(".pane-ov")!
        .classList.toggle("hidden", btn.dataset.pane !== "ov");
      card
        .querySelector(".pane-detail")!
        .classList.toggle("hidden", btn.dataset.pane !== "detail");
    });

    const chart = makeChart(card.querySelector(".gpu-chart") as HTMLElement, [
      { color: "#c084fc", fill: "rgba(192,132,252,0.12)" },
      { color: "#facc15", dash: [4, 4] },
    ]);
    const kv: Record<string, HTMLElement> = {};
    card.querySelectorAll("[data-kv]").forEach((el) => {
      kv[(el as HTMLElement).dataset.kv!] = el as HTMLElement;
    });
    gpuCards.push({
      name: g.name,
      chart,
      utilEl: card.querySelector(".gpu-util") as HTMLElement,
      vramEl: card.querySelector(".gpu-vram") as HTMLElement,
      tempEl: card.querySelector(".gpu-temp") as HTMLElement,
      powerEl: card.querySelector(".gpu-power") as HTMLElement,
      coreEl: card.querySelector(".gpu-core") as HTMLElement,
      fanEl: card.querySelector(".gpu-fan") as HTMLElement,
      badgeThermal: card.querySelector(".t-thermal") as HTMLElement,
      badgePower: card.querySelector(".t-power") as HTMLElement,
      kv,
    });
  });
}

// ---------- 主流程 ----------

async function main() {
  try {
    const info = await invoke<StaticInfo>("get_static_info");
    const cores = info.physical_cores
      ? `${info.physical_cores}C/${info.logical_cores}T`
      : `${info.logical_cores}T`;
    $("static-info").textContent =
      `${info.cpu_name} · ${cores} · ${fmtBytes(info.total_mem)} · ${info.os}`;
    coreClasses = info.core_classes;
    buildPeGroups(info.logical_cores);
  } catch (e) {
    $("static-info").textContent = `系统信息获取失败: ${e}`;
  }

  const cpuChart = makeChart($("cpu-chart"), [
    { color: "#38bdf8", fill: "rgba(56,189,248,0.12)" },
  ]);
  const memChart = makeChart($("mem-chart"), [
    { color: "#34d399", fill: "rgba(52,211,153,0.12)" },
  ]);
  const netChart = makeChart(
    $("net-chart"),
    [
      { color: "#60a5fa", fill: "rgba(96,165,250,0.12)" },
      { color: "#f472b6", dash: [4, 4] },
    ],
    { floor: 10 * 1024, fmt: fmtRate },
  );
  const diskChart = makeChart(
    $("disk-chart"),
    [
      { color: "#2dd4bf", fill: "rgba(45,212,191,0.12)" },
      { color: "#fbbf24", dash: [4, 4] },
    ],
    { floor: 10 * 1024 * 1024, fmt: fmtRate },
  );

  await listen<Snapshot>("metrics", (event) => {
    const s = event.payload;

    // GPU 卡片按需（重）建
    if (
      s.gpus.length !== gpuCards.length ||
      s.gpus.some((g, i) => gpuCards[i]?.name !== g.name)
    ) {
      buildGpuCards(s.gpus);
    }

    timestamps.push(s.ts / 1000);
    cpuTotal.push(s.cpu.total);
    memPct.push((s.mem.used / s.mem.total) * 100);
    s.gpus.forEach((g, i) => {
      gpuUtil[i].push(g.util_pct);
      gpuVramPct[i].push(g.vram_total > 0 ? (g.vram_used / g.vram_total) * 100 : NaN);
    });
    netDown.push(s.net.down_bps);
    netUp.push(s.net.up_bps);
    diskRead.push(s.storage.disks.reduce((a, d) => a + d.read_bps, 0));
    diskWrite.push(s.storage.disks.reduce((a, d) => a + d.write_bps, 0));
    if (timestamps.length > MAX_POINTS) {
      timestamps.shift();
      cpuTotal.shift();
      memPct.shift();
      gpuUtil.forEach((a) => a.shift());
      gpuVramPct.forEach((a) => a.shift());
      netDown.shift();
      netUp.shift();
      diskRead.shift();
      diskWrite.shift();
    }

    const th = thresholds();

    // CPU 数值区
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

    // 内存数值区
    const pct = (s.mem.used / s.mem.total) * 100;
    const memEl = $("mem-pct");
    memEl.textContent = `${pct.toFixed(1)}%`;
    setWarn(memEl, pct >= th.mem);
    $("mem-used").textContent = `${fmtBytes(s.mem.used)} / ${fmtBytes(s.mem.total)}`;
    $("mem-avail").textContent = fmtBytes(s.mem.available);
    $("mem-commit").textContent =
      s.mem.commit_limit > 0
        ? `${fmtBytes(s.mem.commit_used)} / ${fmtBytes(s.mem.commit_limit)}`
        : "N/A";
    const faultsEl = $("mem-faults");
    faultsEl.textContent = `${s.mem.hard_faults_ps.toFixed(0)}/s`;
    setWarn(faultsEl, s.mem.hard_faults_ps > 200);

    // 内存详情页
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

    // GPU 数值区
    s.gpus.forEach((g, i) => {
      const c = gpuCards[i];
      c.utilEl.textContent = `${g.util_pct.toFixed(0)}%`;
      setWarn(c.utilEl, g.util_pct >= th.gpu);
      c.vramEl.textContent =
        g.vram_total > 0
          ? `${fmtBytes(g.vram_used)} / ${fmtBytes(g.vram_total)}`
          : fmtBytes(g.vram_used);
      c.tempEl.textContent = g.temp_c != null ? `${g.temp_c}°C` : "N/A";
      setWarn(c.tempEl, g.temp_c != null && g.temp_c >= th.gpuTemp);
      // 功耗显示为 当前/上限，接近功耗墙（≥95%）时高亮
      if (g.power_w != null && g.power_limit_w != null) {
        c.powerEl.textContent = `${g.power_w.toFixed(0)}/${g.power_limit_w.toFixed(0)} W`;
        setWarn(c.powerEl, g.power_w >= g.power_limit_w * 0.95);
      } else {
        c.powerEl.textContent = g.power_w != null ? `${g.power_w.toFixed(0)} W` : "N/A";
      }
      c.coreEl.textContent =
        g.core_mhz != null
          ? `${g.core_mhz}${g.mem_mhz != null ? `/${g.mem_mhz}` : ""} MHz`
          : "N/A";
      c.fanEl.textContent = g.fan_pct != null ? `${g.fan_pct}%` : "N/A";

      // 节流徽章（硬件级标志）
      c.badgeThermal.classList.toggle("hidden", !g.throttle_thermal);
      c.badgePower.classList.toggle("hidden", !g.throttle_power);

      // 详情页
      const setKv = (key: string, text: string, warn = false) => {
        c.kv[key].textContent = text;
        c.kv[key].classList.toggle("warn", warn);
      };
      setKv(
        "hotspot",
        g.hotspot_c != null ? `${g.hotspot_c.toFixed(0)}°C` : "N/A",
        g.hotspot_c != null && g.temp_slowdown_c != null && g.hotspot_c >= g.temp_slowdown_c - 5,
      );
      setKv("vramtemp", g.vram_temp_c != null ? `${g.vram_temp_c.toFixed(0)}°C` : "N/A");
      setKv("slowdown", g.temp_slowdown_c != null ? `${g.temp_slowdown_c}°C` : "N/A");
      setKv(
        "fan2",
        g.fan_pct != null
          ? `${g.fan_pct}%${g.fan_rpm != null ? ` · ${g.fan_rpm.toFixed(0)} RPM` : ""}`
          : "N/A",
      );
      setKv(
        "memctrl",
        g.mem_ctrl_pct != null ? `${g.mem_ctrl_pct}%` : "N/A",
        g.mem_ctrl_pct != null && g.mem_ctrl_pct >= 90,
      );
      setKv(
        "codec",
        g.enc_pct != null || g.dec_pct != null
          ? `${g.enc_pct ?? 0}% / ${g.dec_pct ?? 0}%`
          : "N/A",
      );
      setKv("pcierx", g.pcie_rx_kbs != null ? fmtRate(g.pcie_rx_kbs * 1024) : "N/A");
      setKv("pcietx", g.pcie_tx_kbs != null ? fmtRate(g.pcie_tx_kbs * 1024) : "N/A");
    });

    // 磁盘数值区
    const diskActive = s.storage.disks.reduce((m, d) => Math.max(m, d.active_pct), 0);
    const activeEl = $("disk-active");
    activeEl.textContent = `${diskActive.toFixed(0)}%`;
    setWarn(activeEl, diskActive >= 90);
    $("disk-read").textContent = fmtRate(diskRead[diskRead.length - 1] ?? 0);
    $("disk-write").textContent = fmtRate(diskWrite[diskWrite.length - 1] ?? 0);
    $("disk-vols").textContent = s.storage.volumes
      .map((v) => `${v.mount.replace(/\\$/, "")} 可用${fmtBytes(v.available)}`)
      .join(" · ");
    $("disk-temps").textContent = s.storage_temps
      .filter((t) => t.temp != null)
      .map((t) => `${t.name.split(" ").slice(-1)[0]} ${t.temp!.toFixed(0)}°`)
      .join(" · ");
    renderDiskDetail(s);

    // 进程 Top-N
    renderProcs($("top-cpu"), s.top_cpu, (p) => `${p.cpu_pct.toFixed(1)}%`);
    renderProcs($("top-mem"), s.top_mem, (p) => fmtBytes(p.mem));
    renderProcs($("top-net"), s.top_net, (p) => fmtRate(p.down_bps + p.up_bps));
    renderProcs($("top-disk"), s.top_disk, (p) => fmtRate(p.disk_bps));
    renderProcs(
      $("top-gpu"),
      s.top_gpu,
      (p) => `${p.gpu_pct.toFixed(0)}% · ${fmtBytes(p.vram)}`,
    );

    // 网络数值区
    $("net-down").textContent = fmtRate(s.net.down_bps);
    $("net-up").textContent = fmtRate(s.net.up_bps);
    $("net-iface").textContent = s.net.ifaces[0]?.name ?? "--";
    const pingEl = $("net-ping");
    if (s.net.ping.active) {
      pingEl.textContent =
        s.net.ping.rtt_ms != null
          ? `${s.net.ping.rtt_ms.toFixed(0)}ms ±${s.net.ping.jitter_ms.toFixed(1)}`
          : "超时";
      pingEl.title = `目标 ${s.net.ping.target} · 均值 ${s.net.ping.avg_ms.toFixed(1)}ms`;
      setWarn(pingEl, s.net.ping.rtt_ms == null || s.net.ping.rtt_ms > 100);
    } else {
      pingEl.textContent = "--";
    }
    const lossEl = $("net-loss");
    lossEl.textContent = s.net.ping.active ? `${s.net.ping.loss_pct.toFixed(0)}%` : "--";
    setWarn(lossEl, s.net.ping.loss_pct > 2);
    renderNetDetail(s);

    // 每核心条（核心分组页）
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

    // 频率页
    fqMax = Math.max(fqMax, s.cpu.freq_mhz);
    if (s.cpu.freq_mhz > 0) {
      fqMin = Math.min(fqMin, s.cpu.freq_mhz);
      fqSum += s.cpu.freq_mhz;
      fqN += 1;
    }
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

    // 电源页
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

    // 曲线
    const start = windowStart();
    const ts = timestamps.slice(start);
    cpuChart.setData([ts, cpuTotal.slice(start)]);
    memChart.setData([ts, memPct.slice(start)]);
    gpuCards.forEach((c, i) => {
      c.chart.setData([ts, gpuUtil[i].slice(start), gpuVramPct[i].slice(start)]);
    });
    netChart.setData([ts, netDown.slice(start), netUp.slice(start)]);
    diskChart.setData([ts, diskRead.slice(start), diskWrite.slice(start)]);
  });

  // ---------- 会话记录 ----------
  let recording = false;
  let recStartedAt = 0;

  function renderRecordBtn(samples?: number) {
    const btn = $("record-btn");
    btn.classList.toggle("recording", recording);
    if (recording) {
      const secs = Math.max(0, Math.floor((Date.now() - recStartedAt) / 1000));
      const mm = String(Math.floor(secs / 60)).padStart(2, "0");
      const ss = String(secs % 60).padStart(2, "0");
      btn.textContent = `■ ${mm}:${ss}${samples != null ? ` · ${samples}` : ""}`;
    } else {
      btn.textContent = "● 记录";
    }
  }

  async function syncRecStatus() {
    const st = await invoke<RecStatus>("recording_status");
    recording = st.active;
    recStartedAt = st.started_at ?? 0;
    renderRecordBtn(st.samples);
  }
  await syncRecStatus();
  setInterval(() => recording && syncRecStatus(), 1000);

  $("record-btn").addEventListener("click", async () => {
    await invoke(recording ? "stop_recording" : "start_recording");
    // 采样线程在下个周期真正开/关会话，先乐观更新按钮
    recording = !recording;
    recStartedAt = Date.now();
    renderRecordBtn();
  });

  // ---------- 会话列表与导出 ----------
  const modal = $("sessions-modal");
  const toast = $("export-toast");

  function fmtTime(ms: number): string {
    return new Date(ms).toLocaleString("zh-CN", { hour12: false });
  }

  function fmtDur(a: number, b: number | null): string {
    if (b == null) return "进行中";
    const s = Math.max(0, Math.floor((b - a) / 1000));
    return `${Math.floor(s / 3600)}时${Math.floor((s % 3600) / 60)}分${s % 60}秒`;
  }

  async function refreshSessions() {
    const list = $("sessions-list");
    const sessions = await invoke<SessionMeta[]>("list_sessions");
    if (sessions.length === 0) {
      list.innerHTML = `<div class="sessions-empty">暂无会话，点击顶栏"● 记录"开始录制</div>`;
      return;
    }
    list.innerHTML = "";
    for (const s of sessions) {
      const row = document.createElement("div");
      row.className = "session-row";
      row.innerHTML = `
        <div class="session-info">
          <span>#${s.id} · ${fmtTime(s.started_at)}</span>
          <small>${fmtDur(s.started_at, s.ended_at)} · ${s.samples} 个采样</small>
        </div>
        <div class="session-actions">
          <button data-fmt="html">HTML</button>
          <button data-fmt="csv">CSV</button>
          <button data-fmt="json">JSON</button>
          <button data-fmt="md">摘要</button>
          <button data-fmt="__del" class="danger">删除</button>
        </div>`;
      row.querySelector(".session-actions")!.addEventListener("click", async (e) => {
        const btn = (e.target as HTMLElement).closest("button");
        if (!btn) return;
        const fmt = btn.dataset.fmt!;
        try {
          if (fmt === "__del") {
            await invoke("delete_session", { sessionId: s.id });
            await refreshSessions();
            return;
          }
          const path = await invoke<string>("export_report", {
            sessionId: s.id,
            format: fmt,
          });
          toast.textContent = `已导出：${path}（点击定位文件）`;
          toast.classList.remove("hidden", "error");
          toast.onclick = () => invoke("open_in_folder", { path });
        } catch (err) {
          toast.textContent = `导出失败：${err}`;
          toast.classList.remove("hidden");
          toast.classList.add("error");
          toast.onclick = null;
        }
      });
      list.appendChild(row);
    }
  }

  $("sessions-btn").addEventListener("click", async () => {
    toast.classList.add("hidden");
    modal.classList.remove("hidden");
    await refreshSessions();
  });
  $("modal-close").addEventListener("click", () => modal.classList.add("hidden"));
  modal.addEventListener("click", (e) => {
    if (e.target === modal) modal.classList.add("hidden");
  });

  // 无边框窗口控制键
  for (const [id, action] of [
    ["win-min", "min"],
    ["win-max", "max"],
    ["win-close", "close"],
  ] as const) {
    $(id).addEventListener("click", () => invoke("window_control", { action }));
  }

  // 悬浮窗显隐
  $("overlay-toggle").addEventListener("click", async () => {
    const visible = await invoke<boolean>("toggle_overlay");
    $("overlay-toggle").classList.toggle("active", visible);
  });

  // 置顶
  let onTop = false;
  $("ontop-toggle").addEventListener("click", async () => {
    onTop = !onTop;
    await invoke("set_main_on_top", { on: onTop });
    $("ontop-toggle").classList.toggle("active", onTop);
  });

  // 紧凑模式
  let compact = false;
  $("compact-toggle").addEventListener("click", async () => {
    compact = !compact;
    document.body.classList.toggle("compact", compact);
    $("compact-toggle").classList.toggle("active", compact);
    await invoke("set_compact", { on: compact });
  });

  // ---------- 设置 ----------
  const settingsModal = $("settings-modal");
  const thInputs: Record<string, keyof typeof DEFAULTS> = {
    "th-cpu": "cpu",
    "th-mem": "mem",
    "th-gpu": "gpu",
    "th-cpu-temp": "cpuTemp",
    "th-gpu-temp": "gpuTemp",
  };

  function fillSettings() {
    const th = thresholds();
    for (const [id, key] of Object.entries(thInputs)) {
      ($(id) as HTMLInputElement).value = String(th[key]);
    }
  }

  $("settings-btn").addEventListener("click", async () => {
    fillSettings();
    settingsModal.classList.remove("hidden");
    ($("autostart-chk") as HTMLInputElement).checked =
      await invoke<boolean>("get_autostart");
  });
  $("settings-close").addEventListener("click", () =>
    settingsModal.classList.add("hidden"),
  );
  settingsModal.addEventListener("click", (e) => {
    if (e.target === settingsModal) settingsModal.classList.add("hidden");
  });

  for (const [id, key] of Object.entries(thInputs)) {
    $(id).addEventListener("change", (e) => {
      const v = Number((e.target as HTMLInputElement).value);
      if (Number.isFinite(v) && v > 0) saveThresholds({ [key]: v });
    });
  }
  $("th-reset").addEventListener("click", () => {
    saveThresholds({ ...DEFAULTS });
    fillSettings();
  });
  // 延迟探测目标（持久化并在启动时恢复）
  const PING_KEY = "sysscope-ping-target";
  const savedTarget = localStorage.getItem(PING_KEY);
  if (savedTarget) {
    ($("ping-target") as HTMLInputElement).value = savedTarget;
    void invoke("set_ping_target", { target: savedTarget });
  }
  $("ping-target").addEventListener("change", async (e) => {
    const v = (e.target as HTMLInputElement).value.trim();
    if (v) {
      localStorage.setItem(PING_KEY, v);
      await invoke("set_ping_target", { target: v });
    }
  });

  $("autostart-chk").addEventListener("change", async (e) => {
    const chk = e.target as HTMLInputElement;
    try {
      chk.checked = await invoke<boolean>("set_autostart", {
        enable: chk.checked,
      });
    } catch {
      chk.checked = !chk.checked;
    }
  });

  // CPU 卡片标签页切换
  $("cpu-tabs").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (!btn) return;
    document
      .querySelectorAll("#cpu-tabs button")
      .forEach((b) => b.classList.toggle("active", b === btn));
    for (const pane of ["overview", "cores", "freq", "power"]) {
      $(`cpu-pane-${pane}`).classList.toggle("hidden", pane !== btn.dataset.pane);
    }
  });

  // 进程详情弹窗关闭
  $("pd-close").addEventListener("click", closeProcDetail);
  $("proc-modal").addEventListener("click", (e) => {
    if (e.target === $("proc-modal")) closeProcDetail();
  });

  // 内存 / 磁盘 / 网络卡片标签页切换
  for (const prefix of ["mem", "disk", "net"]) {
    $(`${prefix}-tabs`).addEventListener("click", (e) => {
      const btn = (e.target as HTMLElement).closest("button");
      if (!btn) return;
      document
        .querySelectorAll(`#${prefix}-tabs button`)
        .forEach((b) => b.classList.toggle("active", b === btn));
      $(`${prefix}-pane-ov`).classList.toggle("hidden", btn.dataset.pane !== "ov");
      $(`${prefix}-pane-detail`).classList.toggle(
        "hidden",
        btn.dataset.pane !== "detail",
      );
    });
  }

  // 时间窗切换
  $("window-seg").addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (!btn) return;
    windowSec = Number(btn.dataset.win);
    document
      .querySelectorAll("#window-seg button")
      .forEach((b) => b.classList.toggle("active", b === btn));
  });

  // 采样间隔切换
  $("interval-seg").addEventListener("click", async (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (!btn) return;
    await invoke("set_sample_interval", { ms: Number(btn.dataset.ms) });
    document
      .querySelectorAll("#interval-seg button")
      .forEach((b) => b.classList.toggle("active", b === btn));
  });
}

main();
