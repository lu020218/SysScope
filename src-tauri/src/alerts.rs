//! 阈值告警：持续超限判定与通知。
//!
//! 判定放在**采样线程**而不是前端，尽管阈值本身是前端配置的。原因是主窗口
//! 隐藏到托盘后 WebView 会被 Chromium 节流（我们还主动把它降到低内存态），
//! 而"用户没在看面板"恰恰是最需要告警的场景 —— 把判定交给一个可能被挂起的
//! WebView，等于在最需要它工作的时候最不可靠。
//!
//! 前端只负责把阈值下发过来（同 ping::set_ping_target 的模式）。

use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// 单条告警记录（用于通知文案、录制入库与报告时间线）
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Alert {
    /// 指标 i18n key，如 "alert.metric.cpuTemp"
    pub metric: String,
    pub value: f64,
    pub threshold: f64,
    /// 单位原样透传（"%"、"°C"），不翻译
    pub unit: String,
}

/// camelCase 以对齐前端 thresholds() 返回的对象形状，前端无需为下发再转一次
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AlertConfig {
    pub enabled: bool,
    /// 需持续超限多少秒才告警。0 表示立即
    pub dwell_secs: u64,
    pub cpu: f64,
    pub mem: f64,
    pub gpu: f64,
    pub cpu_temp: f64,
    pub gpu_temp: f64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        // 与前端 thresholds.ts 的 DEFAULTS 保持一致；前端就绪后会立即下发覆盖
        AlertConfig {
            enabled: true,
            dwell_secs: 15,
            cpu: 90.0,
            mem: 90.0,
            gpu: 90.0,
            cpu_temp: 95.0,
            gpu_temp: 85.0,
        }
    }
}

static CONFIG: LazyLock<Mutex<AlertConfig>> =
    LazyLock::new(|| Mutex::new(AlertConfig::default()));

/// 前端下发告警配置（阈值与前端 UI 共用同一套数值）
#[tauri::command]
pub fn set_alert_config(config: AlertConfig) {
    *CONFIG.lock().unwrap() = config;
}

/// 解除告警的回差：低于阈值这么多才算恢复。
/// 没有回差的话，数值在阈值附近抖动会反复触发通知。
const HYSTERESIS: f64 = 5.0;

#[derive(Default)]
struct MetricState {
    over_since: Option<Instant>,
    fired: bool,
}

impl MetricState {
    /// 返回 Some 表示此刻应当触发一次告警
    fn update(&mut self, value: f64, threshold: f64, dwell: Duration) -> bool {
        if value >= threshold {
            let since = *self.over_since.get_or_insert_with(Instant::now);
            if !self.fired && since.elapsed() >= dwell {
                self.fired = true;
                return true;
            }
        } else if value < threshold - HYSTERESIS {
            // 回差之外才真正复位，避免临界抖动导致反复通知
            self.over_since = None;
            self.fired = false;
        }
        false
    }
}

/// 从快照中抽出的待判定读数。把它与 Snapshot 解耦，判定逻辑就能脱离
/// 完整快照单独测试 —— 否则为了构造测试数据要给八个结构体加 Default，
/// 反而让"随手造个空快照"在生产代码里也变得容易。
#[derive(Default, Clone, Copy)]
pub struct Readings {
    pub cpu: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub mem: Option<f64>,
    pub gpu: Option<f64>,
    pub gpu_temp: Option<f64>,
}

#[derive(Default)]
pub struct AlertWatcher {
    cpu: MetricState,
    mem: MetricState,
    gpu: MetricState,
    cpu_temp: MetricState,
    gpu_temp: MetricState,
}

impl AlertWatcher {
    /// 每拍调用；返回本拍新触发的告警（通常为空）
    pub fn tick(&mut self, s: &crate::sampler::Snapshot) -> Vec<Alert> {
        // 多卡时取负载/温度最高的一块：任意一块出问题都值得提醒。
        // 空集合 fold 出 NaN，恰好表示"这台机器没有这项读数"
        let max_of = |it: &mut dyn Iterator<Item = f64>| {
            it.fold(f64::NAN, f64::max).pipe_finite()
        };
        self.evaluate(Readings {
            cpu: Some(s.cpu.total as f64),
            cpu_temp: s.cpu.temp_c.map(|v| v as f64),
            mem: (s.mem.total > 0)
                .then(|| s.mem.used as f64 / s.mem.total as f64 * 100.0),
            gpu: max_of(&mut s.gpus.iter().map(|g| g.util_pct as f64)),
            gpu_temp: max_of(&mut s.gpus.iter().filter_map(|g| g.temp_c).map(|v| v as f64)),
        })
    }

    pub fn evaluate(&mut self, r: Readings) -> Vec<Alert> {
        let cfg = CONFIG.lock().unwrap().clone();
        if !cfg.enabled {
            return Vec::new();
        }
        let dwell = Duration::from_secs(cfg.dwell_secs);
        let mut out = Vec::new();
        let mut check =
            |state: &mut MetricState, value: Option<f64>, threshold: f64, metric: &str, unit: &str| {
                let Some(v) = value else { return };
                if state.update(v, threshold, dwell) {
                    out.push(Alert {
                        metric: metric.into(),
                        value: v,
                        threshold,
                        unit: unit.into(),
                    });
                }
            };
        check(&mut self.cpu, r.cpu, cfg.cpu, "alert.metric.cpu", "%");
        check(&mut self.cpu_temp, r.cpu_temp, cfg.cpu_temp, "alert.metric.cpuTemp", "°C");
        check(&mut self.mem, r.mem, cfg.mem, "alert.metric.mem", "%");
        check(&mut self.gpu, r.gpu, cfg.gpu, "alert.metric.gpu", "%");
        check(&mut self.gpu_temp, r.gpu_temp, cfg.gpu_temp, "alert.metric.gpuTemp", "°C");
        out
    }
}

/// fold 出来的 NaN 表示"没有这项读数"，转成 None
trait FiniteExt {
    fn pipe_finite(self) -> Option<f64>;
}
impl FiniteExt for f64 {
    fn pipe_finite(self) -> Option<f64> {
        self.is_finite().then_some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dwell(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn fires_only_after_dwell_elapses() {
        let mut st = MetricState::default();
        // 刚超限不该立刻告警
        assert!(!st.update(95.0, 90.0, dwell(60)));
        assert!(!st.update(95.0, 90.0, dwell(60)));
        // dwell 为 0 时应当立即触发
        let mut now = MetricState::default();
        assert!(now.update(95.0, 90.0, dwell(0)));
    }

    #[test]
    fn fires_once_per_excursion() {
        let mut st = MetricState::default();
        assert!(st.update(95.0, 90.0, dwell(0)));
        // 持续超限不重复轰炸
        assert!(!st.update(96.0, 90.0, dwell(0)));
        assert!(!st.update(99.0, 90.0, dwell(0)));
    }

    #[test]
    fn hysteresis_prevents_flapping() {
        let mut st = MetricState::default();
        assert!(st.update(95.0, 90.0, dwell(0)));
        // 落回阈值之下但仍在回差区间内：不复位，因此不会再次触发
        assert!(!st.update(88.0, 90.0, dwell(0)));
        assert!(!st.update(95.0, 90.0, dwell(0)));
        // 真正回落到回差之外才复位
        assert!(!st.update(84.0, 90.0, dwell(0)));
        assert!(st.update(95.0, 90.0, dwell(0)));
    }

    #[test]
    fn disabled_config_emits_nothing() {
        set_alert_config(AlertConfig {
            enabled: false,
            ..AlertConfig::default()
        });
        let mut w = AlertWatcher::default();
        let alerts = w.evaluate(Readings {
            cpu: Some(100.0),
            cpu_temp: Some(120.0),
            ..Readings::default()
        });
        assert!(alerts.is_empty());
        set_alert_config(AlertConfig::default());
    }

    #[test]
    fn missing_readings_never_alert() {
        set_alert_config(AlertConfig {
            dwell_secs: 0,
            ..AlertConfig::default()
        });
        let mut w = AlertWatcher::default();
        // 没有温度传感器的机器上 cpu_temp 恒为 None，不该被当成 0 或误报
        let alerts = w.evaluate(Readings::default());
        assert!(alerts.is_empty());
        set_alert_config(AlertConfig::default());
    }

    #[test]
    fn alert_carries_metric_key_and_unit() {
        set_alert_config(AlertConfig {
            dwell_secs: 0,
            cpu_temp: 95.0,
            ..AlertConfig::default()
        });
        let mut w = AlertWatcher::default();
        let alerts = w.evaluate(Readings {
            cpu_temp: Some(97.5),
            ..Readings::default()
        });
        assert_eq!(alerts.len(), 1);
        // metric 是 i18n key 而非本地化文案，翻译在前端/报告侧完成
        assert_eq!(alerts[0].metric, "alert.metric.cpuTemp");
        assert_eq!(alerts[0].unit, "°C");
        assert_eq!(alerts[0].threshold, 95.0);
        set_alert_config(AlertConfig::default());
    }
}

/// 弹出系统通知。失败静默忽略 —— 通知权限被关、或运行在未安装的
/// 开发构建下（Windows 的 toast 需要已注册的 AppUserModelID）时，
/// 不应该影响监控本身。
pub fn notify(app: &tauri::AppHandle, alert: &Alert) {
    use tauri_plugin_notification::NotificationExt;
    let lang = crate::i18n::current();
    let body = crate::i18n::tr_fmt(
        lang,
        "alert.body",
        &[
            ("metric", crate::i18n::tr(lang, &alert.metric)),
            ("value", &format!("{:.0}", alert.value)),
            ("threshold", &format!("{:.0}", alert.threshold)),
            ("unit", &alert.unit),
        ],
    );
    let _ = app
        .notification()
        .builder()
        .title(crate::i18n::tr(lang, "alert.title"))
        .body(body)
        .show();
}
