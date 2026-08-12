import uPlot from "uplot";

export interface SeriesDef {
  color: string;
  fill?: string;
  dash?: number[];
}

export interface YCfg {
  /** 固定 0-100 百分比轴 */
  pct?: boolean;
  /** 自动轴的最小上界 */
  floor?: number;
  /** 自定义刻度格式 */
  fmt?: (v: number) => string;
}

export function makeChart(
  el: HTMLElement,
  series: SeriesDef[],
  y: YCfg = { pct: true },
): uPlot {
  const opts: uPlot.Options = {
    width: el.clientWidth || 400,
    height: el.clientHeight || 150,
    padding: [8, 8, 0, 0],
    cursor: { show: false },
    scales: {
      x: { time: true },
      y: y.pct
        ? { range: [0, 100] }
        : { range: (_u, _min, max) => [0, Math.max(y.floor ?? 10, (max || 0) * 1.15)] },
    },
    axes: [
      {
        stroke: "#69748a",
        grid: { stroke: "#1b222d", width: 1 },
        ticks: { show: false },
        font: "10px Segoe UI",
      },
      {
        stroke: "#69748a",
        grid: { stroke: "#1b222d", width: 1 },
        ticks: { show: false },
        font: "10px Segoe UI",
        size: y.fmt ? 58 : 36,
        values: (_u, vals) =>
          vals.map((v) => (y.fmt ? y.fmt(v) : y.pct ? `${v}%` : `${v}`)),
      },
    ],
    series: [
      {},
      ...series.map((s) => ({
        stroke: s.color,
        fill: s.fill,
        width: 1.5,
        dash: s.dash,
        points: { show: false },
      })),
    ],
  };
  const chart = new uPlot(opts, [[], ...series.map(() => [])] as uPlot.AlignedData, el);
  new ResizeObserver(() => {
    chart.setSize({ width: el.clientWidth, height: el.clientHeight });
  }).observe(el);
  return chart;
}

/**
 * 滚动缓冲区：统一管理时间轴与各命名系列的 push/对齐/修剪，
 * 消除逐数组手工同步。新系列自动以 NaN 回填对齐历史长度。
 */
export class RingBuffers {
  readonly ts: number[] = [];
  private data = new Map<string, number[]>();

  constructor(private readonly cap = 4000) {}

  /** 提交一帧：values 中缺失的已注册系列补 NaN */
  commit(tsSec: number, values: Record<string, number>) {
    for (const name of Object.keys(values)) {
      if (!this.data.has(name)) {
        this.data.set(name, new Array(this.ts.length).fill(NaN));
      }
    }
    this.ts.push(tsSec);
    for (const [name, arr] of this.data) {
      arr.push(values[name] ?? NaN);
    }
    if (this.ts.length > this.cap) {
      this.ts.shift();
      for (const arr of this.data.values()) {
        arr.shift();
      }
    }
  }

  series(name: string): number[] {
    return this.data.get(name) ?? [];
  }

  /** 当前时间窗对应的起始下标 */
  windowStart(windowSec: number): number {
    const cutoff = Date.now() / 1000 - windowSec;
    const i = this.ts.findIndex((t) => t >= cutoff);
    return i < 0 ? this.ts.length : i;
  }

  /** 删除某前缀的全部系列（GPU 卡片重建时清理旧键） */
  dropPrefix(prefix: string) {
    for (const k of [...this.data.keys()]) {
      if (k.startsWith(prefix)) {
        this.data.delete(k);
      }
    }
  }
}

/** 全局唯一的实时数据缓冲（30min @ 0.5s ≈ 3600 点，留余量） */
export const buffers = new RingBuffers(4000);
