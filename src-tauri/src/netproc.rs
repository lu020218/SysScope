use ferrisetw::parser::Parser;
use ferrisetw::provider::Provider;
use ferrisetw::trace::UserTrace;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, System};

/// Microsoft-Windows-Kernel-Network：TCP/UDP 收发事件（含 PID 与字节数）
const KERNEL_NETWORK: &str = "7DD42A49-5329-4832-8DFD-43D979153A88";
/// 事件 ID：TCPv4 发送/接收、UDPv4 发送/接收、TCPv6/UDPv6 对应事件
const EV_TCP_SEND: u16 = 10;
const EV_TCP_RECV: u16 = 11;
const EV_TCP6_SEND: u16 = 26;
const EV_TCP6_RECV: u16 = 27;
const EV_UDP_SEND: u16 = 42;
const EV_UDP_RECV: u16 = 43;
const EV_UDP6_SEND: u16 = 58;
const EV_UDP6_RECV: u16 = 59;

const SESSION_PREFIX: &str = "SysScopeNetProc";
const TOP_N: usize = 5;

#[derive(Serialize, Clone)]
pub struct ProcNetStat {
    pub pid: u32,
    pub name: String,
    pub down_bps: f64,
    pub up_bps: f64,
}

#[derive(Default)]
struct Accum {
    /// pid -> (接收字节, 发送字节)，每个采样周期由 drain 清零
    bytes: HashMap<u32, (u64, u64)>,
}

pub struct NetProcCollector {
    state: Arc<Mutex<Accum>>,
    pub available: bool,
    _trace: Option<UserTrace>,
}

impl NetProcCollector {
    pub fn init() -> Self {
        // 孤儿会话清理由 SamplerCtx 启动时统一异步执行

        let state: Arc<Mutex<Accum>> = Arc::default();
        let s = state.clone();
        let provider = Provider::by_guid(KERNEL_NETWORK)
            .add_callback(
                move |record: &ferrisetw::EventRecord,
                      locator: &ferrisetw::schema_locator::SchemaLocator| {
                    let id = record.event_id();
                    let is_recv = matches!(id, EV_TCP_RECV | EV_TCP6_RECV | EV_UDP_RECV | EV_UDP6_RECV);
                    let is_send = matches!(id, EV_TCP_SEND | EV_TCP6_SEND | EV_UDP_SEND | EV_UDP6_SEND);
                    if !is_recv && !is_send {
                        return;
                    }
                    let Ok(schema) = locator.event_schema(record) else {
                        return;
                    };
                    let parser = Parser::create(record, &schema);
                    let (Ok(pid), Ok(size)) =
                        (parser.try_parse::<u32>("PID"), parser.try_parse::<u32>("size"))
                    else {
                        return;
                    };
                    let mut acc = s.lock().unwrap();
                    let e = acc.bytes.entry(pid).or_default();
                    if is_recv {
                        e.0 += size as u64;
                    } else {
                        e.1 += size as u64;
                    }
                },
            )
            .build();

        match UserTrace::new()
            .named(crate::etw_util::session_name(SESSION_PREFIX))
            .enable(provider)
            .start_and_process()
        {
            Ok(trace) => NetProcCollector {
                state,
                available: true,
                _trace: Some(trace),
            },
            Err(e) => {
                eprintln!("[sysscope] per-process network ETW init failed: {e:?}");
                NetProcCollector {
                    state,
                    available: false,
                    _trace: None,
                }
            }
        }
    }

    /// 取走累计字节并折算为速率，返回按总速率排序的 Top-N；
    /// 进程名从 sys 已刷新的进程表中查找
    pub fn sample(&self, sys: &System, elapsed_secs: f64) -> Vec<ProcNetStat> {
        if !self.available {
            return Vec::new();
        }
        let elapsed = if elapsed_secs > 0.0 { elapsed_secs } else { 1.0 };
        let drained: HashMap<u32, (u64, u64)> =
            std::mem::take(&mut self.state.lock().unwrap().bytes);
        let mut stats: Vec<ProcNetStat> = drained
            .into_iter()
            .filter(|(pid, (rx, tx))| *pid != 0 && rx + tx > 0)
            .map(|(pid, (rx, tx))| ProcNetStat {
                pid,
                name: sys
                    .process(Pid::from_u32(pid))
                    .map(|p| p.name().to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("PID {pid}")),
                down_bps: rx as f64 / elapsed,
                up_bps: tx as f64 / elapsed,
            })
            .collect();
        stats.sort_by(|a, b| {
            (b.down_bps + b.up_bps)
                .partial_cmp(&(a.down_bps + a.up_bps))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        stats.truncate(TOP_N);
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 需要管理员权限；启动会话后产生一点网络流量，验证能按 PID 聚合到字节
    #[test]
    #[ignore = "hw: 需要管理员（ETW 内核网络）与出网流量"]
    fn collects_per_process_bytes() {
        let collector = NetProcCollector::init();
        if !collector.available {
            println!("net ETW unavailable (no admin), skipping");
            return;
        }
        // 触发一次真实网络请求（DNS 查询即可产生 UDP 流量）
        let _ = std::net::TcpStream::connect_timeout(
            &"223.5.5.5:80".parse().unwrap(),
            std::time::Duration::from_secs(2),
        );
        std::thread::sleep(std::time::Duration::from_secs(3));
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let stats = collector.sample(&sys, 3.0);
        println!(
            "per-process net: {:?}",
            stats
                .iter()
                .map(|s| format!("{} ↓{:.0} ↑{:.0}", s.name, s.down_bps, s.up_bps))
                .collect::<Vec<_>>()
        );
        assert!(
            !stats.is_empty(),
            "expected some per-process network traffic"
        );
    }
}
