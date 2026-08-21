import { $, setWarn } from "../format";
import { t } from "../i18n";
import { FIXED } from "../thresholds";
import type { Snapshot } from "../types";

/**
 * 主板卡片：SuperIO 报出的机箱风扇转速与板载温度点。
 *
 * 这张卡片没有曲线（数据 5 秒才刷新一次，画出来是阶梯不是曲线），因此用
 * 数值瓦片而非图表 —— 与卡片头部的 stat 是同一套视觉语言：大数字 + 小标签。
 *
 * 传感器命名完全由主板决定（"Fan #2"、"Chipset"、"VRM MOS"……），各家不同，
 * 因此原样显示厂商给的名字，不做映射 —— 猜错名字比不猜更糟。
 */

/** 渲染一组瓦片；仅在内容变化时重建 DOM，避免每拍重排 */
function renderTiles(
  el: HTMLElement,
  items: { name: string; value: number }[],
  fmt: (v: number) => string,
  warnAbove?: number,
) {
  const sig = items.map((i) => `${i.name}=${fmt(i.value)}`).join("|");
  if (el.dataset.sig === sig) return;
  el.dataset.sig = sig;

  el.innerHTML = "";
  for (const it of items) {
    const tile = document.createElement("div");
    tile.className = "tile";
    const v = document.createElement("b");
    v.textContent = fmt(it.value);
    if (warnAbove != null && it.value >= warnAbove) tile.classList.add("warn");
    const n = document.createElement("span");
    // 传感器名来自主板固件，走 textContent
    n.textContent = it.name;
    tile.append(v, n);
    el.appendChild(tile);
  }
}

export function update(s: Snapshot) {
  const card = $("board-card");
  const b = s.board;
  // 采不到主板传感器的机器（无 SuperIO 支持、或未提权）直接隐藏整张卡片，
  // 而不是留一张全是 N/A 的空卡
  card.classList.toggle("hidden", !b || (b.fans.length === 0 && b.temps.length === 0));
  if (!b) return;

  // 型号放进标题，与 GPU 卡片"GPU · 型号"一致；采不到时退回纯标题
  $("board-title").textContent = b.name
    ? `${t("card.board")} · ${b.name}`
    : t("card.board");

  const hottest = b.temps.reduce((m, x) => Math.max(m, x.value), -1);
  const hotEl = $("board-hottest");
  hotEl.textContent = hottest >= 0 ? `${hottest.toFixed(0)}°C` : "--";
  setWarn(hotEl, hottest >= FIXED.boardTempC);

  // 头部显示最高转速而非风扇数量：数量是静态事实，转速才是要盯的读数
  const topFan = b.fans.reduce((m, f) => Math.max(m, f.value), -1);
  $("board-fans").textContent = topFan >= 0 ? `${topFan.toFixed(0)} RPM` : "--";

  $("board-fan-tiles").parentElement!.classList.toggle("hidden", b.fans.length === 0);
  $("board-temp-tiles").parentElement!.classList.toggle("hidden", b.temps.length === 0);
  renderTiles($("board-fan-tiles"), b.fans, (v) => `${v.toFixed(0)}`);
  renderTiles($("board-temp-tiles"), b.temps, (v) => `${v.toFixed(0)}°`, FIXED.boardTempC);
}
