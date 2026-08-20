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

/** 非用户可配的固定告警阈值（全部集中于此，勿在卡片内散写魔数） */
export const FIXED = {
  /** 主板温度点告警线（VRM/芯片组普遍比 CPU 低，80°C 已属偏高） */
  boardTempC: 80,
  /** 硬页面错误率（次/秒），超过视为内存换页压力 */
  hardFaultsPs: 200,
  /** 提交内存占提交上限比例 % */
  commitPct: 90,
  /** 磁盘活动时间 % */
  diskActivePct: 90,
  /** 延迟探测 RTT（ms）与丢包率 % */
  pingRttMs: 100,
  pingLossPct: 2,
  /** GPU 功耗达到功耗墙的比例 */
  gpuPowerWallRatio: 0.95,
  /** GPU 显存控制器负载 % */
  gpuMemCtrlPct: 90,
} as const;

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

/**
 * 报告是否输出完整序列号。默认关闭 —— 主板/磁盘序列号与 MAC 能唯一标识
 * 这台机器，而报告是拿来分享的；界面上完整显示，导出时才脱敏。
 */
const FULL_SERIALS_KEY = "sysscope-full-serials";

export function fullSerials(): boolean {
  return localStorage.getItem(FULL_SERIALS_KEY) === "1";
}

export function setFullSerials(on: boolean) {
  localStorage.setItem(FULL_SERIALS_KEY, on ? "1" : "0");
}

/** 告警设置。判定在后端进行，这里只负责持久化与下发 */
const ALERT_KEY = "sysscope-alerts";

export interface AlertPrefs {
  enabled: boolean;
  /** 需持续超限多少秒才通知；0 为立即 */
  dwellSecs: number;
}

export const ALERT_DEFAULTS: AlertPrefs = { enabled: true, dwellSecs: 15 };

export function alertPrefs(): AlertPrefs {
  try {
    return { ...ALERT_DEFAULTS, ...JSON.parse(localStorage.getItem(ALERT_KEY) ?? "{}") };
  } catch {
    return { ...ALERT_DEFAULTS };
  }
}

export function saveAlertPrefs(patch: Partial<AlertPrefs>) {
  localStorage.setItem(ALERT_KEY, JSON.stringify({ ...alertPrefs(), ...patch }));
}
