import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "uplot/dist/uPlot.min.css";
import "./styles.css";

import * as cpu from "./cards/cpu";
import * as disk from "./cards/disk";
import * as gpu from "./cards/gpu";
import * as mem from "./cards/mem";
import * as net from "./cards/net";
import * as procs from "./cards/procs";
import { buffers } from "./charts";
import { $, fmtBytes } from "./format";
import * as procdetail from "./modals/procdetail";
import * as sessions from "./modals/sessions";
import * as settings from "./modals/settings";
import { initAllTabs } from "./tabs";
import { thresholds } from "./thresholds";
import type { Snapshot, StaticInfo } from "./types";

/** 曲线时间窗（秒），顶栏可切换 */
let windowSec = 60;

async function main() {
  initAllTabs();
  procdetail.init();
  settings.init();
  await sessions.init();

  // 静态信息 + CPU 拓扑
  let info: StaticInfo | null = null;
  try {
    info = await invoke<StaticInfo>("get_static_info");
    const cores = info.physical_cores
      ? `${info.physical_cores}C/${info.logical_cores}T`
      : `${info.logical_cores}T`;
    $("static-info").textContent =
      `${info.cpu_name} · ${cores} · ${fmtBytes(info.total_mem)} · ${info.os}`;
  } catch (e) {
    $("static-info").textContent = `系统信息获取失败: ${e}`;
  }
  cpu.init(info);
  mem.init();
  disk.init();
  net.init();

  // 采样线程异常警示：崩溃时显示，恢复出数后自动隐藏
  await listen("sampler-crashed", () => {
    $("crash-warn").classList.remove("hidden");
  });

  // 实时数据分发：缓冲 → 各卡片 update
  await listen<Snapshot>("metrics", (event) => {
    const s = event.payload;
    $("crash-warn").classList.add("hidden");

    const values: Record<string, number> = {
      cpu: s.cpu.total,
      mem: (s.mem.used / s.mem.total) * 100,
      down: s.net.down_bps,
      up: s.net.up_bps,
      dread: s.storage.disks.reduce((a, d) => a + d.read_bps, 0),
      dwrite: s.storage.disks.reduce((a, d) => a + d.write_bps, 0),
    };
    gpu.bufferValues(s, values);
    buffers.commit(s.ts / 1000, values);

    const th = thresholds();
    const start = buffers.windowStart(windowSec);
    const ts = buffers.ts.slice(start);

    cpu.update(s, th, ts, start);
    mem.update(s, th, ts, start);
    gpu.update(s, th, ts, start);
    disk.update(s, ts, start);
    net.update(s, ts, start);
    procs.update(s);
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
