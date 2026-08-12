/** 告警阈值：localStorage 持久化，主窗口与悬浮窗（同源）共享；
 *  其他窗口修改时通过 storage 事件自动刷新缓存 */
export interface Thresholds {
  cpu: number;
  mem: number;
  gpu: number;
  cpuTemp: number;
  gpuTemp: number;
}

export const DEFAULTS: Thresholds = {
  cpu: 90,
  mem: 90,
  gpu: 90,
  cpuTemp: 95,
  gpuTemp: 85,
};

const KEY = "sysscope-thresholds";

function load(): Thresholds {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return { ...DEFAULTS };
  }
}

let cache: Thresholds = load();

export function thresholds(): Thresholds {
  return cache;
}

export function saveThresholds(t: Partial<Thresholds>) {
  cache = { ...cache, ...t };
  localStorage.setItem(KEY, JSON.stringify(cache));
}

addEventListener("storage", () => {
  cache = load();
});
