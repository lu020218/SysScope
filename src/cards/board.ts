import { $, addInfoBlock, setWarn } from "../format";
import { t } from "../i18n";
import { FIXED } from "../thresholds";
import type { Snapshot } from "../types";

/**
 * 主板卡片：SuperIO 报出的机箱风扇转速与板载温度点。
 *
 * 传感器命名完全由主板决定（"Fan #2"、"Chipset"、"VRM MOS"……），各家不同，
 * 因此原样显示厂商给的名字，不做映射 —— 猜错名字比不猜更糟。
 * 数据本身低频刷新（见 sampler 的 BOARD_REFRESH）。
 */
export function update(s: Snapshot) {
  const card = $("board-card");
  const b = s.board;
  // 采不到主板传感器的机器（无 SuperIO 支持、或未提权）直接隐藏整张卡片，
  // 而不是留一张全是 N/A 的空卡
  card.classList.toggle("hidden", !b || (b.fans.length === 0 && b.temps.length === 0));
  if (!b) return;

  $("board-fans").textContent = b.fans.length ? String(b.fans.length) : "--";
  const hottest = b.temps.reduce((m, x) => Math.max(m, x.value), -1);
  const hotEl = $("board-hottest");
  hotEl.textContent = hottest >= 0 ? `${hottest.toFixed(0)}°C` : "--";
  setWarn(hotEl, hottest >= FIXED.boardTempC);

  const wrap = $("board-detail");
  wrap.innerHTML = "";
  if (b.fans.length) {
    addInfoBlock(
      wrap,
      t("board.fans"),
      b.fans.map((f) => [f.name, `${f.value.toFixed(0)} RPM`] as [string, string]),
    );
  }
  if (b.temps.length) {
    addInfoBlock(
      wrap,
      t("board.temps"),
      b.temps.map((x) => [x.name, `${x.value.toFixed(1)} °C`] as [string, string]),
    );
  }
}
