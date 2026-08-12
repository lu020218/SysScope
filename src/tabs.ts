/**
 * 卡片标签页统一接线：约定 .tabs 内 button[data-pane] 切换
 * 同卡片内 .tab-pane[data-pane]；激活面板记录在卡片 dataset.pane，
 * 供更新逻辑跳过隐藏面板的 DOM 渲染。
 */

export function wireTabs(card: HTMLElement) {
  const tabs = card.querySelector(".tabs");
  if (!tabs) return;
  const active = tabs.querySelector("button.active") as HTMLElement | null;
  card.dataset.pane = active?.dataset.pane ?? "";
  tabs.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest("button");
    if (!btn?.dataset.pane) return;
    tabs
      .querySelectorAll("button")
      .forEach((b) => b.classList.toggle("active", b === btn));
    card.querySelectorAll(".tab-pane").forEach((p) => {
      const pane = p as HTMLElement;
      pane.classList.toggle("hidden", pane.dataset.pane !== btn.dataset.pane);
    });
    card.dataset.pane = btn.dataset.pane;
  });
}

/** 当前激活面板名（无标签页的卡片返回空串） */
export function activePane(card: HTMLElement): string {
  return card.dataset.pane ?? "";
}

/** 静态卡片统一接线（动态创建的 GPU 卡片自行调用 wireTabs） */
export function initAllTabs() {
  document.querySelectorAll<HTMLElement>(".card").forEach(wireTabs);
}
