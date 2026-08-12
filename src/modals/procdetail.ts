import { invoke } from "@tauri-apps/api/core";
import { getCoreClasses } from "../cards/cpu";
import { $, fmtBytes, fmtCpuTime } from "../format";
import type { ProcDetail } from "../types";

let timer: number | null = null;

function renderAffinity(mask: number) {
  const wrap = $("pd-affinity");
  wrap.innerHTML = "";
  const n = Math.max(getCoreClasses().length, 1);
  for (let i = 0; i < n; i++) {
    const dot = document.createElement("span");
    dot.className = "aff-dot";
    // 53 位内安全；掩码高于核心数的位不展示
    if (i < 53 && (mask / 2 ** i) % 2 >= 1) dot.classList.add("on");
    dot.title = `C${i}`;
    wrap.appendChild(dot);
  }
}

export function close() {
  if (timer != null) {
    clearInterval(timer);
    timer = null;
  }
  $("proc-modal").classList.add("hidden");
}

export function open(pid: number, name: string) {
  close();
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
  timer = window.setInterval(tick, 1000);
}

export function init() {
  $("pd-close").addEventListener("click", close);
  $("proc-modal").addEventListener("click", (e) => {
    if (e.target === $("proc-modal")) close();
  });
}
