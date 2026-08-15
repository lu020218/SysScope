import { $, fmtBytes, fmtRate } from "../format";
import { t } from "../i18n";
import { open as openProcDetail } from "../modals/procdetail";
import type { Snapshot } from "../types";

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
    name.title = t("procs.rowTitle", { pid: p.pid });
    const val = document.createElement("span");
    val.className = "proc-val";
    val.textContent = fmt(p);
    row.append(name, val);
    row.addEventListener("click", () => openProcDetail(p.pid, p.name));
    el.appendChild(row);
  }
}

export function update(s: Snapshot) {
  renderProcs($("top-cpu"), s.top_cpu, (p) => `${p.cpu_pct.toFixed(1)}%`);
  renderProcs($("top-mem"), s.top_mem, (p) => fmtBytes(p.mem));
  renderProcs($("top-net"), s.top_net, (p) => fmtRate(p.down_bps + p.up_bps));
  renderProcs($("top-disk"), s.top_disk, (p) => fmtRate(p.disk_bps));
  renderProcs(
    $("top-gpu"),
    s.top_gpu,
    (p) => `${p.gpu_pct.toFixed(0)}% · ${fmtBytes(p.vram)}`,
  );
}
