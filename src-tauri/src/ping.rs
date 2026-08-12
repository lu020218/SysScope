use serde::Serialize;
use std::collections::VecDeque;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
};

/// 默认探测目标（阿里公共 DNS，国内低延迟）
pub const DEFAULT_TARGET: &str = "223.5.5.5";
/// 统计窗口：最近 N 次探测
const WINDOW: usize = 60;
const PROBE_INTERVAL: Duration = Duration::from_secs(1);
const TIMEOUT_MS: u32 = 1000;

#[derive(Serialize, Clone, Default)]
pub struct PingStats {
    pub target: String,
    /// 最近一次 RTT（ms），超时为 None
    pub rtt_ms: Option<f64>,
    pub avg_ms: f64,
    /// 抖动：相邻成功探测 RTT 差的平均绝对值
    pub jitter_ms: f64,
    pub loss_pct: f64,
    /// 探测线程是否在工作（目标解析失败等时为 false）
    pub active: bool,
}

/// 探测目标：全局可配置（前端 set_ping_target 命令写入，探测线程每轮读取）
static TARGET: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(DEFAULT_TARGET.to_string()));

#[tauri::command]
pub fn set_ping_target(target: String) {
    let t = target.trim();
    if !t.is_empty() {
        *TARGET.lock().unwrap() = t.to_string();
    }
}

#[derive(Default)]
struct PingState {
    history: VecDeque<Option<f64>>,
    last_target: String,
    active: bool,
}

pub struct PingProber {
    state: Arc<Mutex<PingState>>,
}

fn resolve(target: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = target.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    (target, 0u16)
        .to_socket_addrs()
        .ok()?
        .find_map(|a| match a.ip() {
            std::net::IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
}

/// 单次 ICMP 探测，返回 RTT（ms），失败/超时为 None
fn probe_once(ip: Ipv4Addr) -> Option<f64> {
    unsafe {
        let handle = IcmpCreateFile().ok()?;
        let payload = [0x53u8; 32]; // 'S'
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
        let n = IcmpSendEcho(
            handle,
            u32::from_ne_bytes(ip.octets()),
            payload.as_ptr() as *const _,
            payload.len() as u16,
            None,
            reply.as_mut_ptr() as *mut _,
            reply.len() as u32,
            TIMEOUT_MS,
        );
        let _ = IcmpCloseHandle(handle);
        if n == 0 {
            return None;
        }
        let echo = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (echo.Status == 0).then_some(echo.RoundTripTime as f64)
    }
}

impl PingProber {
    pub fn spawn() -> Self {
        let state: Arc<Mutex<PingState>> = Arc::default();
        let s = state.clone();
        std::thread::spawn(move || loop {
            let tgt = TARGET.lock().unwrap().clone();
            let ip = resolve(&tgt);
            let rtt = ip.and_then(probe_once);
            {
                let mut st = s.lock().unwrap();
                // 目标切换时清空历史
                if st.last_target != tgt {
                    st.history.clear();
                    st.last_target = tgt;
                }
                st.active = ip.is_some();
                if ip.is_some() {
                    st.history.push_back(rtt);
                    if st.history.len() > WINDOW {
                        st.history.pop_front();
                    }
                }
            }
            std::thread::sleep(PROBE_INTERVAL);
        });
        PingProber { state }
    }

    pub fn stats(&self) -> PingStats {
        let st = self.state.lock().unwrap();
        let succ: Vec<f64> = st.history.iter().flatten().copied().collect();
        let avg = if succ.is_empty() {
            0.0
        } else {
            succ.iter().sum::<f64>() / succ.len() as f64
        };
        let jitter = if succ.len() < 2 {
            0.0
        } else {
            succ.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (succ.len() - 1) as f64
        };
        let loss = if st.history.is_empty() {
            0.0
        } else {
            st.history.iter().filter(|r| r.is_none()).count() as f64 / st.history.len() as f64
                * 100.0
        };
        PingStats {
            target: st.last_target.clone(),
            rtt_ms: st.history.back().copied().flatten(),
            avg_ms: avg,
            jitter_ms: jitter,
            loss_pct: loss,
            active: st.active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_localhost_succeeds() {
        // 本机回环必然可达且 RTT 极小
        let rtt = probe_once(Ipv4Addr::new(127, 0, 0, 1));
        println!("localhost rtt: {rtt:?}");
        assert!(rtt.is_some());
        assert!(rtt.unwrap() < 100.0);
    }

    #[test]
    fn resolver_handles_ip_and_bad_host() {
        assert_eq!(resolve("223.5.5.5"), Some(Ipv4Addr::new(223, 5, 5, 5)));
        assert!(resolve("definitely-not-a-real-host.invalid").is_none());
    }

    #[test]
    fn prober_collects_stats() {
        set_ping_target("127.0.0.1".into());
        let p = PingProber::spawn();
        std::thread::sleep(Duration::from_millis(2500));
        let s = p.stats();
        println!(
            "target={} rtt={:?} avg={} jitter={} loss={}%",
            s.target, s.rtt_ms, s.avg_ms, s.jitter_ms, s.loss_pct
        );
        assert!(s.active);
        assert!(s.rtt_ms.is_some());
        assert_eq!(s.loss_pct, 0.0);
    }
}
