/**
 * 顶层视图切换：主页 / 进程 / 硬件。
 *
 * 与卡片内标签页同一条约定 —— 隐藏的视图不渲染。主页收起时六张卡片的
 * DOM 更新与图表重绘全部跳过，只保留 RingBuffers 的数据写入，这样切回来
 * 曲线是连续的，而不是从头画起。
 */

export type View = "home" | "procs" | "hardware";

let current: View = "home";
const listeners: ((v: View) => void)[] = [];

export function activeView(): View {
  return current;
}

/** 视图切换回调（硬件页用它触发首次采集） */
export function onViewChange(fn: (v: View) => void) {
  listeners.push(fn);
}

function apply(v: View) {
  current = v;
  for (const el of document.querySelectorAll<HTMLElement>(".view")) {
    el.classList.toggle("hidden", el.dataset.view !== v);
  }
  for (const b of document.querySelectorAll<HTMLElement>("#view-tabs button")) {
    b.classList.toggle("active", b.dataset.view === v);
  }
  // 曲线时间窗只作用于主页图表，其余视图下这个控件没有意义
  document.getElementById("window-seg")?.classList.toggle("hidden", v !== "home");
  for (const fn of listeners) fn(v);
}

export function initViews() {
  document.getElementById("view-tabs")?.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (btn?.dataset.view) apply(btn.dataset.view as View);
  });
  apply(current);
}
