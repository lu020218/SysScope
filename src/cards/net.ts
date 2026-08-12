import type uPlot from "uplot";
import { buffers, makeChart } from "../charts";
import { $, addInfoBlock, fmtLinkSpeed, fmtRate, setWarn } from "../format";
import { activePane } from "../tabs";
import { FIXED } from "../thresholds";
import type { Snapshot } from "../types";

let chart: uPlot;
let card: HTMLElement;

export function init() {
  card = $("net-card");
  chart = makeChart(
    $("net-chart"),
    [
      { color: "#60a5fa", fill: "rgba(96,165,250,0.12)" },
      { color: "#f472b6", dash: [4, 4] },
    ],
    { floor: 10 * 1024, fmt: fmtRate },
  );
}

export function update(s: Snapshot, ts: number[], start: number) {
  $("net-down").textContent = fmtRate(s.net.down_bps);
  $("net-up").textContent = fmtRate(s.net.up_bps);

  const pane = activePane(card);

  if (pane === "ov") {
    $("net-iface").textContent = s.net.ifaces[0]?.name ?? "--";
    const pingEl = $("net-ping");
    if (s.net.ping.active) {
      pingEl.textContent =
        s.net.ping.rtt_ms != null
          ? `${s.net.ping.rtt_ms.toFixed(0)}ms ±${s.net.ping.jitter_ms.toFixed(1)}`
          : "超时";
      pingEl.title = `目标 ${s.net.ping.target} · 均值 ${s.net.ping.avg_ms.toFixed(1)}ms`;
      setWarn(
        pingEl,
        s.net.ping.rtt_ms == null || s.net.ping.rtt_ms > FIXED.pingRttMs,
      );
    } else {
      pingEl.textContent = "--";
    }
    const lossEl = $("net-loss");
    lossEl.textContent = s.net.ping.active
      ? `${s.net.ping.loss_pct.toFixed(0)}%`
      : "--";
    setWarn(lossEl, s.net.ping.loss_pct > FIXED.pingLossPct);
    chart.setData([
      ts,
      buffers.series("down").slice(start),
      buffers.series("up").slice(start),
    ]);
  }

  if (pane === "detail") {
    renderDetail(s);
  }
}

/** 网络详情页：连接统计 + 重传 + 各网卡链路利用率 */
function renderDetail(s: Snapshot) {
  const wrap = $("net-detail");
  wrap.innerHTML = "";
  addInfoBlock(wrap, "TCP / UDP 连接", [
    ["已建立", String(s.net.tcp_established)],
    ["TIME_WAIT", String(s.net.tcp_time_wait)],
    ["监听", String(s.net.tcp_listen)],
    ["UDP 端点", String(s.net.udp_endpoints)],
  ]);
  addInfoBlock(wrap, "TCP 重传", [
    ["重传", `${s.net.retrans_ps.toFixed(0)}/s`],
    ["重传率", `${s.net.retrans_pct.toFixed(2)}%`],
  ]);
  for (const a of s.net.adapters) {
    addInfoBlock(wrap, a.name, [
      ["链路", fmtLinkSpeed(a.link_bps)],
      ["利用率", `${a.util_pct.toFixed(1)}%`],
    ]);
  }
}
