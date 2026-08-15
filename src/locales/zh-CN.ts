/**
 * 简体中文语言包 —— 基准语言：key 集合以此为准，其余语言包由 TS 校验完整性。
 * key 命名：模块.区块.条目
 */
const zh = {
  // 通用
  "common.close": "关闭",

  // 顶栏与工具条
  "app.loading": "加载系统信息…",
  "app.crashWarn": "采集线程发生异常，已自动重启，数据可能有短暂缺口",
  "win.onTop": "窗口置顶",
  "win.settings": "设置",
  "win.min": "最小化",
  "win.max": "最大化 / 还原",
  "win.close": "关闭到托盘",
  "toolbar.record.title": "开始/停止记录监控会话",
  "toolbar.sessions": "报告",
  "toolbar.sessions.title": "查看会话并导出报告",
  "toolbar.overlay": "悬浮窗",
  "toolbar.overlay.title": "显示/隐藏 FPS 悬浮窗",
  "toolbar.interval.title": "采样间隔",

  // 卡片标题与通用标签页
  "card.mem": "内存",
  "card.net": "网络",
  "card.disk": "磁盘",
  "card.procs": "进程 Top 5",
  "tab.overview": "概览",
  "tab.detail": "详情",

  // CPU
  "cpu.stat.total": "总占用",
  "cpu.stat.temp": "温度",
  "cpu.stat.power": "功耗",
  "cpu.stat.freq": "频率",
  "cpu.tab.cores": "核心",
  "cpu.tab.freq": "频率",
  "cpu.tab.power": "电源",
  "cpu.freq.cur": "当前",
  "cpu.freq.eff": "有效",
  "cpu.freq.base": "基准",
  "cpu.freq.max": "会话峰值",
  "cpu.freq.avg": "均值",
  "cpu.freq.boost": "睿频中",
  "cpu.power.package": "封装功耗",
  "cpu.power.peak": "会话峰值功耗",
  "cpu.power.voltage": "核心电压",
  "cpu.power.boost": "睿频状态",
  "cpu.power.c1": "C1 驻留",
  "cpu.power.c2": "C2 驻留",
  "cpu.power.c3": "C3 驻留",

  // 内存
  "mem.stat.pct": "占用率",
  "mem.stat.used": "已用 / 总量",
  "mem.sub.avail": "可用",
  "mem.sub.commit": "提交",
  "mem.sub.faults": "硬页错误",
  "mem.detail.cached": "已缓存",
  "mem.detail.standby": "备用 / 已修改",
  "mem.detail.pf": "页错误（总 / 硬）",
  "mem.detail.compression": "内存压缩",
  "mem.detail.swap": "页面文件",
  "mem.detail.commitPct": "提交占比",
  "mem.detail.speed": "内存频率",
  "mem.detail.bandwidth": "理论带宽",

  // 网络
  "net.stat.down": "↓ 下载",
  "net.stat.up": "↑ 上传",
  "net.legend.down": "下载",
  "net.legend.up": "上传",
  "net.sub.iface": "接口",
  "net.sub.ping": "延迟",
  "net.sub.loss": "丢包",

  // 磁盘
  "disk.stat.active": "活动",
  "disk.read": "读取",
  "disk.write": "写入",

  // 进程 Top 5 的列头（列很窄，英文需从简）
  "procs.col.mem": "内存",
  "procs.col.net": "网络",
  "procs.col.disk": "磁盘",
  "procs.col.gpu": "GPU · 显存",

  // 进程详情弹窗
  "pd.title": "进程详情",
  "pd.cputime": "累计 CPU 时间",
  "pd.threads": "线程数",
  "pd.handles": "句柄数",
  "pd.ws": "工作集 / 峰值",
  "pd.priv": "私有提交",
  "pd.pf": "累计页错误",
  "pd.prio": "优先级",
  "pd.affinity": "CPU 亲和性",

  // 会话弹窗
  "sessions.title": "监控会话",
  "sessions.openDir": "📁 报告目录",
  "sessions.openDir.title": "打开报告目录",

  // 设置弹窗
  "settings.title": "设置",
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
