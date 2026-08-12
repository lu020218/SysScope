use ferrisetw::provider::Provider;
use ferrisetw::trace::UserTrace;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Pid, ProcessesToUpdate, System};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Microsoft-Windows-DXGI：IDXGISwapChain::Present 起始事件（覆盖 DX10/11/12）
const DXGI_PROVIDER: &str = "CA11C036-0102-4A2D-A6AD-F03CFED5D3C9";
const DXGI_PRESENT_START: u16 = 42;
/// Microsoft-Windows-D3D9：Present 起始事件（旧 D3D9 应用）
const D3D9_PROVIDER: &str = "783ACA0A-790E-4D7F-8451-AA850511C6B9";
const D3D9_PRESENT_START: u16 = 1;

const SESSION_PREFIX: &str = "SysScopeFps";
/// 每进程保留的 Present 时间戳上限（500fps 下约 8 秒）
const MAX_PRESENTS_PER_PID: usize = 4000;
/// 超过该秒数无新事件的进程从表中清除
const STALE_SECS: f64 = 10.0;
/// 1% Low 的统计窗口（秒）
const LOW_WINDOW_SECS: f64 = 5.0;

#[derive(Serialize, Clone, Default)]
pub struct FpsMetrics {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub low_1pct_fps: f32,
    /// 0.1% Low（5 秒窗口内最差 0.1% 帧时间均值对应帧率）
    pub low_01pct_fps: f32,
    /// 帧时间分位（ms，5 秒窗口）
    pub ft_p95_ms: f32,
    pub ft_p99_ms: f32,
    /// 卡顿计数：5 秒窗口内帧时间 > 2×中位数 的帧数
    pub stutters: u32,
}

#[derive(Serialize, Clone)]
pub struct FpsSnapshot {
    /// "ok" | "no_admin" | "failed"
    pub status: &'static str,
    pub pid: u32,
    pub process: String,
    #[serde(flatten)]
    pub metrics: FpsMetrics,
    /// 前台进程近 2 秒内是否有帧提交
    pub has_data: bool,
}

impl FpsSnapshot {
    fn empty(status: &'static str) -> Self {
        FpsSnapshot {
            status,
            pid: 0,
            process: String::new(),
            metrics: FpsMetrics::default(),
            has_data: false,
        }
    }
}

struct PidPresents {
    /// ETW 原始时间戳（时钟单位由会话决定，用 Calib 自校准换算）
    ts: VecDeque<i64>,
    last_arrival: Instant,
}

/// 自校准：测量原始时间戳与墙钟的比例，兼容 QPC / 100ns 系统时间等会话时钟。
/// 现代 Windows 上 QPC 频率与 100ns 时钟通常都是 10MHz，故默认值即近似正确，
/// 校准仅用于修正老硬件上基于 TSC 的高频 QPC。
struct Calib {
    anchor: Option<(i64, Instant)>,
    latest: Option<(i64, Instant)>,
}

impl Calib {
    const DEFAULT_TPS: f64 = 1e7;

    fn new() -> Self {
        Calib {
            anchor: None,
            latest: None,
        }
    }

    fn observe(&mut self, raw: i64) {
        let now = Instant::now();
        if self.anchor.is_none() {
            self.anchor = Some((raw, now));
        }
        self.latest = Some((raw, now));
    }

    fn ticks_per_sec(&self) -> f64 {
        if let (Some((r0, t0)), Some((r1, t1))) = (self.anchor, self.latest) {
            let span = t1.duration_since(t0).as_secs_f64();
            if span > 2.0 && r1 > r0 {
                return (r1 - r0) as f64 / span;
            }
        }
        Self::DEFAULT_TPS
    }
}

#[derive(Default)]
struct FpsState {
    presents: HashMap<u32, PidPresents>,
    calib: Option<Calib>,
}

pub struct FpsCollector {
    state: Arc<Mutex<FpsState>>,
    status: &'static str,
    /// 持有会话句柄保持 ETW 采集存活
    _trace: Option<UserTrace>,
}

fn on_present(state: &Arc<Mutex<FpsState>>, pid: u32, raw_ts: i64) {
    if pid == 0 {
        return;
    }
    let mut st = state.lock().unwrap();
    st.calib.get_or_insert_with(Calib::new).observe(raw_ts);
    let entry = st.presents.entry(pid).or_insert_with(|| PidPresents {
        ts: VecDeque::new(),
        last_arrival: Instant::now(),
    });
    entry.ts.push_back(raw_ts);
    entry.last_arrival = Instant::now();
    if entry.ts.len() > MAX_PRESENTS_PER_PID {
        entry.ts.pop_front();
    }
}

impl FpsCollector {
    pub fn init() -> Self {
        // 清理属主已退出的残留会话；本进程会话名带 PID 后缀避免多进程冲突
        crate::etw_util::cleanup_stale_sessions(SESSION_PREFIX);

        let state: Arc<Mutex<FpsState>> = Arc::default();

        let s1 = state.clone();
        let dxgi = Provider::by_guid(DXGI_PROVIDER)
            .add_callback(move |record: &ferrisetw::EventRecord, _sl: &ferrisetw::schema_locator::SchemaLocator| {
                if record.event_id() == DXGI_PRESENT_START {
                    on_present(&s1, record.process_id(), record.raw_timestamp());
                }
            })
            .build();
        let s2 = state.clone();
        let d3d9 = Provider::by_guid(D3D9_PROVIDER)
            .add_callback(move |record: &ferrisetw::EventRecord, _sl: &ferrisetw::schema_locator::SchemaLocator| {
                if record.event_id() == D3D9_PRESENT_START {
                    on_present(&s2, record.process_id(), record.raw_timestamp());
                }
            })
            .build();

        match UserTrace::new()
            .named(crate::etw_util::session_name(SESSION_PREFIX))
            .enable(dxgi)
            .enable(d3d9)
            .start_and_process()
        {
            Ok(trace) => FpsCollector {
                state,
                status: "ok",
                _trace: Some(trace),
            },
            Err(e) => {
                eprintln!("[sysscope] FPS ETW init failed: {e:?}");
                FpsCollector {
                    state,
                    status: if is_elevated() { "failed" } else { "no_admin" },
                    _trace: None,
                }
            }
        }
    }

    /// 采样：跟踪当前前台窗口进程并计算其帧率指标
    pub fn sample(&self, sys: &mut System) -> FpsSnapshot {
        if self.status != "ok" {
            return FpsSnapshot::empty(self.status);
        }
        let Some(pid) = foreground_pid() else {
            return FpsSnapshot::empty("ok");
        };

        let process = process_name(sys, pid);
        let mut st = self.state.lock().unwrap();
        st.presents
            .retain(|_, p| p.last_arrival.elapsed().as_secs_f64() < STALE_SECS);
        let tps = st
            .calib
            .as_ref()
            .map(|c| c.ticks_per_sec())
            .unwrap_or(Calib::DEFAULT_TPS);

        let Some(p) = st.presents.get(&pid) else {
            return FpsSnapshot {
                status: "ok",
                pid,
                process,
                ..FpsSnapshot::empty("ok")
            };
        };
        // 近 2 秒（墙钟）无事件视为无帧数据，避免残留数据导致数值冻结
        if p.last_arrival.elapsed().as_secs_f64() > 2.0 {
            return FpsSnapshot {
                status: "ok",
                pid,
                process,
                ..FpsSnapshot::empty("ok")
            };
        }

        let metrics = compute_metrics(&p.ts, tps);
        let has_data = metrics.fps > 0.0;
        FpsSnapshot {
            status: "ok",
            pid,
            process,
            metrics,
            has_data,
        }
    }
}

/// 以最新事件为窗口锚点计算指标（对 ETW 缓冲批量刷新不敏感）：
/// - fps / 平均帧时间：最近 1 秒窗口
/// - 1% / 0.1% Low、P95/P99、卡顿计数：最近 5 秒窗口
fn compute_metrics(ts: &VecDeque<i64>, tps: f64) -> FpsMetrics {
    let Some(&newest) = ts.back() else {
        return FpsMetrics::default();
    };
    if ts.len() < 2 {
        return FpsMetrics::default();
    }
    let one_sec = tps as i64;
    let low_win = (LOW_WINDOW_SECS * tps) as i64;

    let in_1s: Vec<i64> = ts
        .iter()
        .rev()
        .take_while(|&&t| newest - t <= one_sec)
        .copied()
        .collect();
    let fps = in_1s.len().saturating_sub(1) as f32
        * (tps / (newest - in_1s.last().copied().unwrap_or(newest)).max(1) as f64) as f32;

    let mut intervals_1s: Vec<i64> = Vec::new();
    let mut intervals_5s: Vec<i64> = Vec::new();
    let mut prev: Option<i64> = None;
    for &t in ts.iter() {
        if newest - t > low_win {
            continue;
        }
        if let Some(pv) = prev {
            let d = t - pv;
            if d > 0 {
                intervals_5s.push(d);
                if newest - t <= one_sec {
                    intervals_1s.push(d);
                }
            }
        }
        prev = Some(t);
    }
    let frame_time_ms = if intervals_1s.is_empty() {
        0.0
    } else {
        let avg = intervals_1s.iter().sum::<i64>() as f64 / intervals_1s.len() as f64;
        (avg / tps * 1000.0) as f32
    };
    if intervals_5s.is_empty() {
        return FpsMetrics {
            fps,
            frame_time_ms,
            ..FpsMetrics::default()
        };
    }

    // 卡顿：帧时间 > 2×中位数（按未排序序列先求中位数）
    let mut sorted = intervals_5s.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let stutters = intervals_5s.iter().filter(|&&d| d > median * 2).count() as u32;

    // 分位（帧时间从高到低）
    intervals_5s.sort_unstable_by(|a, b| b.cmp(a));
    let n = intervals_5s.len();
    let to_ms = |ticks: i64| (ticks as f64 / tps * 1000.0) as f32;
    let ft_p99_ms = to_ms(intervals_5s[(n / 100).min(n - 1)]);
    let ft_p95_ms = to_ms(intervals_5s[(n * 5 / 100).min(n - 1)]);

    // N% Low：最差 N% 帧时间均值对应帧率（不足 1 帧时取最差一帧）
    let low_n = |frac: f64| -> f32 {
        let k = ((n as f64 * frac) as usize).max(1);
        let worst_avg = intervals_5s[..k].iter().sum::<i64>() as f64 / k as f64;
        (tps / worst_avg) as f32
    };

    FpsMetrics {
        fps,
        frame_time_ms,
        low_1pct_fps: low_n(0.01),
        low_01pct_fps: low_n(0.001),
        ft_p95_ms,
        ft_p99_ms,
        stutters,
    }
}

pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

fn process_name(sys: &mut System, pid: u32) -> String {
    let spid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[spid]), true);
    sys.process(spid)
        .map(|p| p.name().to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("PID {pid}"))
}

fn is_elevated() -> bool {
    // 简化判断：能创建 ETW 会话即视为有权限，这里仅用于失败后的提示分类。
    // 通过尝试打开需要管理员的注册表路径的方式成本更高，直接用环境近似判断。
    std::process::Command::new("net")
        .args(["session"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成数据：60fps 稳定帧序列，混入一帧 50ms 卡顿
    #[test]
    fn compute_metrics_on_synthetic_frames() {
        let tps = 1e7;
        let frame = (tps / 60.0) as i64; // ~166667 ticks
        let mut ts = VecDeque::new();
        let mut t = 0i64;
        for i in 0..300 {
            t += if i == 250 { (0.05 * tps) as i64 } else { frame };
            ts.push_back(t);
        }
        let m = compute_metrics(&ts, tps);
        assert!((55.0..=65.0).contains(&m.fps), "fps={}", m.fps);
        assert!(
            (14.0..=19.0).contains(&m.frame_time_ms),
            "frame_time={}",
            m.frame_time_ms
        );
        // 1% Low 应显著低于均值（受 50ms 卡顿帧影响）
        assert!(
            m.low_1pct_fps > 0.0 && m.low_1pct_fps < 55.0,
            "low_1pct={}",
            m.low_1pct_fps
        );
        // 0.1% Low 只取最差一帧（50ms 卡顿）→ 约 20fps
        assert!(
            m.low_01pct_fps > 0.0 && m.low_01pct_fps <= m.low_1pct_fps + 0.1,
            "low_01pct={}",
            m.low_01pct_fps
        );
        // 分位与卡顿：P99 应捕获到卡顿帧或接近正常帧时间，卡顿计数恰为 1
        assert!(m.ft_p95_ms > 0.0 && m.ft_p99_ms >= m.ft_p95_ms);
        assert_eq!(m.stutters, 1, "stutters={}", m.stutters);
    }

    #[test]
    fn compute_metrics_empty_and_single() {
        let empty = compute_metrics(&VecDeque::new(), 1e7);
        assert_eq!(empty.fps, 0.0);
        assert_eq!(empty.low_1pct_fps, 0.0);
        let mut one = VecDeque::new();
        one.push_back(100);
        let single = compute_metrics(&one, 1e7);
        assert_eq!(single.fps, 0.0);
        assert_eq!(single.stutters, 0);
    }

    #[test]
    fn foreground_pid_returns_something_or_none() {
        // 无前台窗口（服务会话）时为 None，否则为有效 PID
        if let Some(pid) = foreground_pid() {
            assert!(pid > 0);
        }
    }

    /// 实测 ETW 会话：需要管理员权限；启动后等待几秒，
    /// 若桌面有任何 DXGI 渲染（DWM/浏览器/终端），应能收到 Present 事件
    #[test]
    fn etw_session_collects_presents() {
        let collector = FpsCollector::init();
        println!("FPS collector status: {}", collector.status);
        if collector.status != "ok" {
            println!("skipping live ETW assertions (no admin)");
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(4));
        let st = collector.state.lock().unwrap();
        let total: usize = st.presents.values().map(|p| p.ts.len()).sum();
        println!(
            "collected {} presents from {} process(es)",
            total,
            st.presents.len()
        );
        for (pid, p) in st.presents.iter() {
            println!("  pid {} -> {} presents", pid, p.ts.len());
        }
        assert!(
            total > 0,
            "expected at least some Present events on a live desktop"
        );
    }
}
