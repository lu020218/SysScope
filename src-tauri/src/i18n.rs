//! 后端只有极少量原生文案需要多语言（托盘菜单、提权弹窗），
//! 用一张静态表即可，不引任何 i18n 依赖。语言状态的权威在前端；
//! 后端默认跟随系统 UI 语言，前端通过 `set_language` 覆盖。
//!
//! 例外：`elevate.rs` 的提权失败弹窗在 Tauri 启动前就要弹，那时既无
//! WebView 也无用户配置可读，只能用系统语言。

use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    ZhCn,
    En,
}

impl Lang {
    /// 解析前端传来的标记；未知值回退英文
    pub fn parse(s: &str) -> Self {
        if s.to_ascii_lowercase().starts_with("zh") {
            Lang::ZhCn
        } else {
            Lang::En
        }
    }
}

/// Windows 显示语言；LANGID 低 10 位是主语言 ID，0x04 = 中文
pub fn sys_lang() -> Lang {
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid & 0x3ff == 0x04 {
        Lang::ZhCn
    } else {
        Lang::En
    }
}

/// 返回值的生命周期绑定到 key，未命中时可以直接把 key 借出去（无需泄漏）
pub fn tr(lang: Lang, key: &str) -> &str {
    match (lang, key) {
        (Lang::ZhCn, "tray.show") => "显示面板",
        (Lang::ZhCn, "tray.record") => "开始 / 停止记录",
        (Lang::ZhCn, "tray.overlay") => "显示 / 隐藏悬浮窗",
        (Lang::ZhCn, "tray.quit") => "退出",
        (Lang::ZhCn, "tray.tooltip") => "SysScope 系统监控",
        (Lang::ZhCn, "elevate.title") => "SysScope",
        (Lang::ZhCn, "elevate.body") => {
            "SysScope 需要管理员权限才能采集 FPS 与温度等指标。\n\n请在 UAC 提示中允许，或右键选择“以管理员身份运行”。"
        }

        (_, "tray.show") => "Show dashboard",
        (_, "tray.record") => "Start / stop recording",
        (_, "tray.overlay") => "Show / hide overlay",
        (_, "tray.quit") => "Quit",
        (_, "tray.tooltip") => "SysScope system monitor",
        (_, "elevate.title") => "SysScope",
        (_, "elevate.body") => {
            "SysScope needs administrator rights to collect FPS and temperature data.\n\nAllow it in the UAC prompt, or right-click the app and choose \"Run as administrator\"."
        }

        // ---------- 报告导出 ----------
        (Lang::ZhCn, "report.langCode") => "zh-CN",
        (Lang::ZhCn, "report.title") => "SysScope 报告 — 会话 #{id}",
        (Lang::ZhCn, "report.print") => "打印 / 另存为 PDF",
        (Lang::ZhCn, "report.statsHeading") => "统计摘要",
        (Lang::ZhCn, "report.threshHeading") => "阈值超限",
        (Lang::ZhCn, "report.chartsHeading") => "历史曲线",
        (Lang::ZhCn, "report.col.metric") => "指标",
        (Lang::ZhCn, "report.col.avg") => "平均",
        (Lang::ZhCn, "report.col.max") => "峰值",
        (Lang::ZhCn, "report.col.min") => "最低",
        (Lang::ZhCn, "report.col.condition") => "条件",
        (Lang::ZhCn, "report.col.exceed") => "超限采样占比",
        (Lang::ZhCn, "report.chart.util") => "占用率（%）",
        (Lang::ZhCn, "report.chart.temp") => "温度（°C）",
        (Lang::ZhCn, "report.chart.net") => "网络（MB/s）",
        (Lang::ZhCn, "report.chart.disk") => "磁盘（MB/s）",
        (Lang::ZhCn, "report.chart.power") => "功耗（W）",
        (Lang::ZhCn, "report.label.mem") => "内存",
        (Lang::ZhCn, "report.label.vram") => "显存",
        (Lang::ZhCn, "report.label.down") => "下载",
        (Lang::ZhCn, "report.label.up") => "上传",
        (Lang::ZhCn, "report.label.read") => "读取",
        (Lang::ZhCn, "report.label.write") => "写入",
        (Lang::ZhCn, "report.meta.start") => "开始",
        (Lang::ZhCn, "report.meta.end") => "结束",
        (Lang::ZhCn, "report.meta.duration") => "时长",
        (Lang::ZhCn, "report.meta.samples") => "采样",
        (Lang::ZhCn, "report.duration") => "{h}时{m}分{s}秒",
        (Lang::ZhCn, "report.fpsProcs") => "FPS 监控进程",
        (Lang::ZhCn, "report.metric.cpuUtil") => "CPU 占用",
        (Lang::ZhCn, "report.metric.cpuTemp") => "CPU 温度",
        (Lang::ZhCn, "report.metric.cpuPower") => "CPU 功耗",
        (Lang::ZhCn, "report.metric.memUtil") => "内存占用",
        (Lang::ZhCn, "report.metric.gpuUtil") => "GPU 占用",
        (Lang::ZhCn, "report.metric.gpuTemp") => "GPU 温度",
        (Lang::ZhCn, "report.metric.gpuPower") => "GPU 功耗",
        (Lang::ZhCn, "report.metric.frameTime") => "帧时间",
        (Lang::ZhCn, "report.metric.stutters") => "卡顿次数/5s",
        (Lang::ZhCn, "report.metric.diskRead") => "磁盘读取",
        (Lang::ZhCn, "report.metric.diskWrite") => "磁盘写入",
        (Lang::ZhCn, "report.metric.diskActive") => "磁盘活动",
        (Lang::ZhCn, "report.metric.netDown") => "下载速率",
        (Lang::ZhCn, "report.metric.netUp") => "上传速率",
        (Lang::ZhCn, "report.th.cpuUtil") => "CPU 占用 ≥ 90%",
        (Lang::ZhCn, "report.th.cpuTemp") => "CPU 温度 ≥ 95°C",
        (Lang::ZhCn, "report.th.memUtil") => "内存占用 ≥ 90%",
        (Lang::ZhCn, "report.th.gpuUtil") => "GPU 占用 ≥ 90%",
        (Lang::ZhCn, "report.th.gpuTemp") => "GPU 温度 ≥ 85°C",
        (Lang::ZhCn, "report.err.noSession") => "会话 {id} 不存在",
        (Lang::ZhCn, "report.err.noData") => "该会话没有采样数据",
        (Lang::ZhCn, "report.err.badFormat") => "未知格式: {fmt}",

        (_, "report.langCode") => "en",
        (_, "report.title") => "SysScope report — session #{id}",
        (_, "report.print") => "Print / save as PDF",
        (_, "report.statsHeading") => "Summary",
        (_, "report.threshHeading") => "Threshold breaches",
        (_, "report.chartsHeading") => "History",
        (_, "report.col.metric") => "Metric",
        (_, "report.col.avg") => "Average",
        (_, "report.col.max") => "Peak",
        (_, "report.col.min") => "Minimum",
        (_, "report.col.condition") => "Condition",
        (_, "report.col.exceed") => "Samples over",
        (_, "report.chart.util") => "Utilisation (%)",
        (_, "report.chart.temp") => "Temperature (°C)",
        (_, "report.chart.net") => "Network (MB/s)",
        (_, "report.chart.disk") => "Disk (MB/s)",
        (_, "report.chart.power") => "Power (W)",
        (_, "report.label.mem") => "Memory",
        (_, "report.label.vram") => "VRAM",
        (_, "report.label.down") => "Down",
        (_, "report.label.up") => "Up",
        (_, "report.label.read") => "Read",
        (_, "report.label.write") => "Write",
        (_, "report.meta.start") => "Started",
        (_, "report.meta.end") => "Ended",
        (_, "report.meta.duration") => "Duration",
        (_, "report.meta.samples") => "Samples",
        (_, "report.duration") => "{h}h {m}m {s}s",
        (_, "report.fpsProcs") => "Tracked processes",
        (_, "report.metric.cpuUtil") => "CPU load",
        (_, "report.metric.cpuTemp") => "CPU temperature",
        (_, "report.metric.cpuPower") => "CPU power",
        (_, "report.metric.memUtil") => "Memory usage",
        (_, "report.metric.gpuUtil") => "GPU load",
        (_, "report.metric.gpuTemp") => "GPU temperature",
        (_, "report.metric.gpuPower") => "GPU power",
        (_, "report.metric.frameTime") => "Frame time",
        (_, "report.metric.stutters") => "Stutters / 5s",
        (_, "report.metric.diskRead") => "Disk read",
        (_, "report.metric.diskWrite") => "Disk write",
        (_, "report.metric.diskActive") => "Disk active",
        (_, "report.metric.netDown") => "Download",
        (_, "report.metric.netUp") => "Upload",
        (_, "report.th.cpuUtil") => "CPU load ≥ 90%",
        (_, "report.th.cpuTemp") => "CPU temperature ≥ 95°C",
        (_, "report.th.memUtil") => "Memory usage ≥ 90%",
        (_, "report.th.gpuUtil") => "GPU load ≥ 90%",
        (_, "report.th.gpuTemp") => "GPU temperature ≥ 85°C",
        (_, "report.err.noSession") => "Session {id} does not exist",
        (_, "report.err.noData") => "This session has no samples",
        (_, "report.err.badFormat") => "Unknown format: {fmt}",

        // 缺 key 时返回 key 本身：宁可显示 key 也不要空白菜单项
        (_, other) => other,
    }
}

/// 带 {name} 占位替换的取值，与前端 i18n.ts 的 t(key, vars) 行为一致
pub fn tr_fmt(lang: Lang, key: &str, vars: &[(&str, &str)]) -> String {
    let mut s = tr(lang, key).to_string();
    for (name, value) in vars {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全部 key；新增翻译时同步加入，测试即覆盖
    const ALL_KEYS: &[&str] = &[
        "tray.show",
        "tray.record",
        "tray.overlay",
        "tray.quit",
        "tray.tooltip",
        "elevate.title",
        "elevate.body",
        "report.langCode",
        "report.title",
        "report.print",
        "report.statsHeading",
        "report.threshHeading",
        "report.chartsHeading",
        "report.col.metric",
        "report.col.avg",
        "report.col.max",
        "report.col.min",
        "report.col.condition",
        "report.col.exceed",
        "report.chart.util",
        "report.chart.temp",
        "report.chart.net",
        "report.chart.disk",
        "report.chart.power",
        "report.label.mem",
        "report.label.vram",
        "report.label.down",
        "report.label.up",
        "report.label.read",
        "report.label.write",
        "report.meta.start",
        "report.meta.end",
        "report.meta.duration",
        "report.meta.samples",
        "report.duration",
        "report.fpsProcs",
        "report.metric.cpuUtil",
        "report.metric.cpuTemp",
        "report.metric.cpuPower",
        "report.metric.memUtil",
        "report.metric.gpuUtil",
        "report.metric.gpuTemp",
        "report.metric.gpuPower",
        "report.metric.frameTime",
        "report.metric.stutters",
        "report.metric.diskRead",
        "report.metric.diskWrite",
        "report.metric.diskActive",
        "report.metric.netDown",
        "report.metric.netUp",
        "report.th.cpuUtil",
        "report.th.cpuTemp",
        "report.th.memUtil",
        "report.th.gpuUtil",
        "report.th.gpuTemp",
        "report.err.noSession",
        "report.err.noData",
        "report.err.badFormat",
    ];

    #[test]
    fn every_key_is_translated_in_both_languages() {
        for key in ALL_KEYS {
            for lang in [Lang::ZhCn, Lang::En] {
                assert_ne!(tr(lang, key), *key, "{key} missing for {lang:?}");
            }
        }
    }

    /// 报告模板把这些值直接塞进 HTML 与 <script> 里的 JS 字符串字面量。
    /// 引号/反斜杠/尖括号会破坏页面结构，在此设一道闸。
    #[test]
    fn report_strings_are_safe_to_embed() {
        for key in ALL_KEYS.iter().filter(|k| k.starts_with("report.")) {
            for lang in [Lang::ZhCn, Lang::En] {
                let v = tr(lang, key);
                assert!(
                    !v.contains(['"', '\\', '<', '>']),
                    "{key} ({lang:?}) contains a character unsafe for HTML/JS embedding: {v}"
                );
            }
        }
    }

    #[test]
    fn tr_fmt_substitutes_placeholders() {
        assert_eq!(
            tr_fmt(Lang::En, "report.err.noSession", &[("id", "7")]),
            "Session 7 does not exist"
        );
        assert_eq!(
            tr_fmt(Lang::En, "report.duration", &[("h", "1"), ("m", "2"), ("s", "3")]),
            "1h 2m 3s"
        );
    }

    #[test]
    fn parse_falls_back_to_english() {
        assert_eq!(Lang::parse("zh-CN"), Lang::ZhCn);
        assert_eq!(Lang::parse("zh"), Lang::ZhCn);
        assert_eq!(Lang::parse("en"), Lang::En);
        assert_eq!(Lang::parse("klingon"), Lang::En);
    }
}
