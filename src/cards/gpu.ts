import type uPlot from "uplot";
import { buffers, makeChart } from "../charts";
import { $, fmtBytes, fmtRate, setWarn } from "../format";
import { applyStatic } from "../i18n";
import { activePane, wireTabs } from "../tabs";
import { FIXED, type Thresholds } from "../thresholds";
import type { GpuSnapshot, Snapshot } from "../types";

interface GpuCard {
  name: string;
  el: HTMLElement;
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
/** 重建去抖：候选签名需连续出现 REBUILD_STABLE_TICKS 次才真正重建，
 *  防止适配器列表瞬时抖动导致每秒整卡重建（DOM/canvas 重排拖垮低端机） */
const REBUILD_STABLE_TICKS = 3;
let currentSig = "";
let pendingSig: string | null = null;
let pendingCount = 0;

function buildGpuCards(gpus: GpuSnapshot[]) {
  for (const c of gpuCards) {
    c.chart.destroy();
  }
  document.querySelectorAll(".gpu-card").forEach((el) => el.remove());
  gpuCards = [];
  buffers.dropPrefix("g");

  const grid = document.querySelector(".grid")!;
  gpus.forEach((g, i) => {
    const card = document.createElement("section");
    card.className = "card gpu-card";
    card.innerHTML = `
      <div class="card-head">
        <h2 class="gpu-title"></h2>
        <div class="head-right">
          <span class="tbadge t-thermal hidden" data-i18n="gpu.badge.thermal">热节流</span>
          <span class="tbadge t-power hidden" data-i18n="gpu.badge.power">功耗墙</span>
          <div class="stats">
            <div class="stat">
              <span class="stat-value gpu-util">--%</span>
              <span class="stat-label" data-i18n="gpu.stat.util">占用</span>
            </div>
            <div class="stat">
              <span class="stat-value gpu-vram">--</span>
              <span class="stat-label" data-i18n="gpu.stat.vram">显存</span>
            </div>
          </div>
        </div>
      </div>
      <div class="tabs">
        <button data-pane="ov" class="active" data-i18n="tab.overview">概览</button>
        <button data-pane="detail" data-i18n="tab.detail">详情</button>
      </div>
      <div class="tab-pane" data-pane="ov">
        <div class="chart gpu-chart"></div>
        <div class="substats">
          <span class="legend legend-util" data-i18n="gpu.legend.util">占用率</span>
          <span class="legend legend-vram" data-i18n="gpu.legend.vram">显存</span>
          <span><span data-i18n="gpu.sub.temp">温度</span> <b class="gpu-temp">--</b></span>
          <span><span data-i18n="gpu.sub.power">功耗</span> <b class="gpu-power">--</b></span>
          <span><span data-i18n="gpu.sub.core">核心</span> <b class="gpu-core">--</b></span>
          <span><span data-i18n="gpu.sub.fan">风扇</span> <b class="gpu-fan">--</b></span>
        </div>
      </div>
      <div class="tab-pane hidden" data-pane="detail">
        <div class="kv-rows kv-2col">
          <div class="kv"><span data-i18n="gpu.detail.hotspot">热点温度</span><b data-kv="hotspot">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.vramTemp">显存温度</span><b data-kv="vramtemp">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.slowdown">降频阈值</span><b data-kv="slowdown">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.fan">风扇</span><b data-kv="fan2">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.memCtrl">显存控制器负载</span><b data-kv="memctrl">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.codec">视频编码 / 解码</span><b data-kv="codec">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.pcieRx">PCIe 接收</span><b data-kv="pcierx">--</b></div>
          <div class="kv"><span data-i18n="gpu.detail.pcieTx">PCIe 发送</span><b data-kv="pcietx">--</b></div>
        </div>
      </div>`;
    // 卡片是运行时插入的，模板里的 data-i18n 需在此翻译
    applyStatic(card);
    // GPU 名称来自驱动（外部字符串），用 textContent 填充避免 HTML 注入
    (card.querySelector(".gpu-title") as HTMLElement).textContent =
      `GPU${gpus.length > 1 ? ` ${i}` : ""} · ${g.name}`;
    grid.insertBefore(card, $("net-card"));
    wireTabs(card);

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
      el: card,
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

/** 采集缓冲键：g{i}u 占用率、g{i}v 显存百分比 */
export function bufferValues(s: Snapshot, values: Record<string, number>) {
  s.gpus.forEach((g, i) => {
    values[`g${i}u`] = g.util_pct;
    values[`g${i}v`] =
      g.vram_total > 0 ? (g.vram_used / g.vram_total) * 100 : NaN;
  });
}

export function update(s: Snapshot, th: Thresholds, ts: number[], start: number) {
  const sig = s.gpus.map((g) => g.name).join("|");
  let rebuilt = false;
  if (sig !== currentSig) {
    if (sig === pendingSig) {
      pendingCount += 1;
    } else {
      pendingSig = sig;
      pendingCount = 1;
    }
    // 首帧（尚无卡片）立即建；其后需要连续稳定才重建
    if (gpuCards.length === 0 || pendingCount >= REBUILD_STABLE_TICKS) {
      buildGpuCards(s.gpus);
      currentSig = sig;
      pendingSig = null;
      pendingCount = 0;
      rebuilt = true;
    }
  } else {
    pendingSig = null;
    pendingCount = 0;
  }

  s.gpus.forEach((g, i) => {
    const c = gpuCards[i];
    if (!c) return; // 去抖期间快照与卡片可能短暂不齐，跳过越界项
    c.utilEl.textContent = `${g.util_pct.toFixed(0)}%`;
    setWarn(c.utilEl, g.util_pct >= th.gpu);
    c.vramEl.textContent =
      g.vram_total > 0
        ? `${fmtBytes(g.vram_used)} / ${fmtBytes(g.vram_total)}`
        : fmtBytes(g.vram_used);
    // 节流徽章（硬件级标志，任何面板下都更新）
    c.badgeThermal.classList.toggle("hidden", !g.throttle_thermal);
    c.badgePower.classList.toggle("hidden", !g.throttle_power);

    const pane = activePane(c.el);

    if (pane === "ov") {
      c.tempEl.textContent = g.temp_c != null ? `${g.temp_c}°C` : "N/A";
      setWarn(c.tempEl, g.temp_c != null && g.temp_c >= th.gpuTemp);
      if (g.power_w != null && g.power_limit_w != null) {
        c.powerEl.textContent = `${g.power_w.toFixed(0)}/${g.power_limit_w.toFixed(0)} W`;
        setWarn(c.powerEl, g.power_w >= g.power_limit_w * FIXED.gpuPowerWallRatio);
      } else {
        c.powerEl.textContent = g.power_w != null ? `${g.power_w.toFixed(0)} W` : "N/A";
      }
      c.coreEl.textContent =
        g.core_mhz != null
          ? `${g.core_mhz}${g.mem_mhz != null ? `/${g.mem_mhz}` : ""} MHz`
          : "N/A";
      c.fanEl.textContent = g.fan_pct != null ? `${g.fan_pct}%` : "N/A";
      // 重建当帧缓冲键刚被清空，跳过一次绘制避免与时间轴长度不齐
      if (!rebuilt) {
        c.chart.setData([
          ts,
          buffers.series(`g${i}u`).slice(start),
          buffers.series(`g${i}v`).slice(start),
        ]);
      }
    }

    if (pane === "detail") {
      const setKv = (key: string, text: string, warn = false) => {
        c.kv[key].textContent = text;
        c.kv[key].classList.toggle("warn", warn);
      };
      setKv(
        "hotspot",
        g.hotspot_c != null ? `${g.hotspot_c.toFixed(0)}°C` : "N/A",
        g.hotspot_c != null &&
          g.temp_slowdown_c != null &&
          g.hotspot_c >= g.temp_slowdown_c - 5,
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
        g.mem_ctrl_pct != null && g.mem_ctrl_pct >= FIXED.gpuMemCtrlPct,
      );
      setKv(
        "codec",
        g.enc_pct != null || g.dec_pct != null
          ? `${g.enc_pct ?? 0}% / ${g.dec_pct ?? 0}%`
          : "N/A",
      );
      setKv("pcierx", g.pcie_rx_kbs != null ? fmtRate(g.pcie_rx_kbs * 1024) : "N/A");
      setKv("pcietx", g.pcie_tx_kbs != null ? fmtRate(g.pcie_tx_kbs * 1024) : "N/A");
    }
  });
}
