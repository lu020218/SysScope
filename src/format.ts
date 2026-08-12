/** DOM 与格式化工具（各卡片/弹窗共用） */

export const $ = (id: string) => document.getElementById(id)!;

export function setWarn(el: HTMLElement, warn: boolean) {
  el.classList.toggle("warn", warn);
}

export function fmtBytes(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

export function fmtRate(bps: number): string {
  if (bps >= 1024 ** 3) return `${(bps / 1024 ** 3).toFixed(2)} GB/s`;
  if (bps >= 1024 ** 2) return `${(bps / 1024 ** 2).toFixed(1)} MB/s`;
  if (bps >= 1024) return `${(bps / 1024).toFixed(0)} KB/s`;
  return `${bps.toFixed(0)} B/s`;
}

export function fmtLinkSpeed(bps: number): string {
  if (bps >= 1e9) return `${(bps / 1e9).toFixed(bps % 1e9 === 0 ? 0 : 1)} Gbps`;
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(0)} Mbps`;
  return `${(bps / 1e3).toFixed(0)} Kbps`;
}

export function fmtCpuTime(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** 通用信息块（磁盘/网络详情页共用）——全部走 DOM API，杜绝注入 */
export function addInfoBlock(
  wrap: HTMLElement,
  name: string,
  pairs: [string, string][],
) {
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
