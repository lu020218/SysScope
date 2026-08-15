/**
 * 简体中文语言包 —— 基准语言：key 集合以此为准，其余语言包由 TS 校验完整性。
 * key 命名：模块.区块.条目
 */
const zh = {
  // 设置弹窗
  "settings.title": "设置",
  "settings.close": "关闭",
  "settings.language": "界面语言",
  "settings.thresholds": "告警阈值",
  "settings.th.cpu": "CPU 占用（%）",
  "settings.th.mem": "内存占用（%）",
  "settings.th.gpu": "GPU 占用（%）",
  "settings.th.cpuTemp": "CPU 温度（°C）",
  "settings.th.gpuTemp": "GPU 温度（°C）",
  "settings.probe": "网络探测",
  "settings.probe.target": "延迟探测目标（IP/域名）",
  "settings.diag": "诊断",
  "settings.diag.sampleCost": "单次采样耗时",
  "settings.reset": "恢复默认",

  // 悬浮窗（窄条，标签必须短）
  "osd.mem": "内存",
  "osd.net": "网络",
};

export type Keys = keyof typeof zh;
export default zh;
