// 窗口尺寸自适应：以 max-content 面板实测尺寸为准
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, PhysicalSize } from "@tauri-apps/api/window";
import "./overlay.css";
import { thresholds } from "./thresholds";

interface Snapshot {
  cpu: { total: number; temp_c: number | null };
  mem: { used: number; total: number };
  gpus: { util_pct: number; temp_c: number | null }[];
  fps: { status: string; fps: number; has_data: boolean };
  net: { down_bps: number; up_bps: number };
}

const $ = (id: string) => document.getElementById(id)!;

function setVal(el: HTMLElement, text: string, warn: boolean) {
  el.textContent = text;
  el.classList.toggle("warn", warn);
}

/** 窗口尺寸随内容自适应（变化超过 2px 才调整，避免抖动）。
 *  注意：必须用物理像素 + devicePixelRatio 显式换算，高分屏（如 150% 缩放）
 *  下 LogicalSize 会被误按物理像素应用导致窗口被压小、内容裁切 */
const win = getCurrentWindow();
let lastW = 0;
let lastH = 0;

async function fitWindow() {
  const panel = document.querySelector(".panel") as HTMLElement;
  const r = panel.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.ceil(r.width * dpr);
  const h = Math.ceil(r.height * dpr);
  if (Math.abs(w - lastW) > 2 * dpr || Math.abs(h - lastH) > 2 * dpr) {
    lastW = w;
    lastH = h;
    await win.setSize(new PhysicalSize(w, h));
  }
}

/** "12% 61°"；无温度时仅 "12%" */
function withTemp(pct: number, temp: number | null): string {
  const p = `${pct.toFixed(0)}%`;
  return temp != null ? `${p} ${temp.toFixed(0)}°` : p;
}

/** 紧凑速率："2.1M" / "56K" / "0" */
function shortRate(bps: number): string {
  if (bps >= 1024 ** 3) return `${(bps / 1024 ** 3).toFixed(1)}G`;
  if (bps >= 1024 ** 2) return `${(bps / 1024 ** 2).toFixed(1)}M`;
  if (bps >= 1024) return `${(bps / 1024).toFixed(0)}K`;
  return `${bps.toFixed(0)}`;
}

listen<Snapshot>("metrics", (event) => {
  const s = event.payload;
  const th = thresholds();

  $("osd-fps").textContent =
    s.fps.status === "ok" && s.fps.has_data ? s.fps.fps.toFixed(0) : "--";

  setVal(
    $("osd-cpu"),
    withTemp(s.cpu.total, s.cpu.temp_c),
    s.cpu.total >= th.cpu || (s.cpu.temp_c != null && s.cpu.temp_c >= th.cpuTemp),
  );

  const gpu = s.gpus[0];
  if (gpu) {
    setVal(
      $("osd-gpu"),
      withTemp(gpu.util_pct, gpu.temp_c),
      gpu.util_pct >= th.gpu || (gpu.temp_c != null && gpu.temp_c >= th.gpuTemp),
    );
  } else {
    $("osd-gpu").textContent = "--";
  }

  const memPct = (s.mem.used / s.mem.total) * 100;
  setVal($("osd-mem"), `${memPct.toFixed(0)}%`, memPct >= th.mem);

  $("osd-net").textContent = `↓${shortRate(s.net.down_bps)} ↑${shortRate(s.net.up_bps)}`;

  void fitWindow();
});
