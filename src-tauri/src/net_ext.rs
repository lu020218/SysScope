use crate::wmi_hub::WmiHub;
use serde::{Deserialize, Serialize};
use windows::Win32::NetworkManagement::IpHelper::{
    GetTcpTable, GetUdpTable, MIB_TCPTABLE, MIB_UDPTABLE,
};

#[derive(Serialize, Clone, Default)]
pub struct AdapterUtil {
    pub name: String,
    /// 协商链路速率（bit/s）
    pub link_bps: u64,
    /// 当前利用率 %（收发合计 ÷ 链路速率）
    pub util_pct: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct NetExt {
    pub tcp_established: u32,
    pub tcp_time_wait: u32,
    pub tcp_listen: u32,
    pub udp_endpoints: u32,
    /// TCP 段重传（次/秒）与重传率 %（重传/发送）
    pub retrans_ps: f64,
    pub retrans_pct: f64,
    pub adapters: Vec<AdapterUtil>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_Tcpip_TCPv4")]
#[serde(rename_all = "PascalCase")]
struct TcpV4 {
    segments_retransmitted_persec: u32,
    segments_sent_persec: u32,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_Tcpip_NetworkInterface")]
#[serde(rename_all = "PascalCase")]
struct NetInterface {
    name: String,
    current_bandwidth: u64,
    bytes_total_persec: u64,
}

/// TCP 状态常量（MIB_TCP_STATE）
const TCP_STATE_LISTEN: u32 = 2;
const TCP_STATE_ESTAB: u32 = 5;
const TCP_STATE_TIME_WAIT: u32 = 11;

/// IPv4 TCP 连接分状态计数：(已建立, TIME_WAIT, 监听)
pub fn tcp_state_counts() -> (u32, u32, u32) {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetTcpTable(None, &mut size, false);
        if size == 0 {
            return (0, 0, 0);
        }
        let mut buf = vec![0u8; size as usize];
        if GetTcpTable(Some(buf.as_mut_ptr() as *mut MIB_TCPTABLE), &mut size, false) != 0 {
            return (0, 0, 0);
        }
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        let (mut estab, mut tw, mut listen) = (0u32, 0u32, 0u32);
        for row in rows {
            match row.Anonymous.dwState {
                TCP_STATE_ESTAB => estab += 1,
                TCP_STATE_TIME_WAIT => tw += 1,
                TCP_STATE_LISTEN => listen += 1,
                _ => {}
            }
        }
        (estab, tw, listen)
    }
}

/// IPv4 UDP 活动端点数
pub fn udp_endpoint_count() -> u32 {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetUdpTable(None, &mut size, false);
        if size == 0 {
            return 0;
        }
        let mut buf = vec![0u8; size as usize];
        if GetUdpTable(Some(buf.as_mut_ptr() as *mut MIB_UDPTABLE), &mut size, false) != 0 {
            return 0;
        }
        (*(buf.as_ptr() as *const MIB_UDPTABLE)).dwNumEntries
    }
}

/// 网络深化：TCP/UDP 连接统计、重传率、各网卡链路利用率
pub fn sample(hub: &WmiHub) -> NetExt {
    let (tcp_established, tcp_time_wait, tcp_listen) = tcp_state_counts();
    let udp_endpoints = udp_endpoint_count();
    let (retrans_ps, retrans_pct) = hub
        .query::<TcpV4>()
        .and_then(|rows| rows.into_iter().next())
        .map(|t| {
            let sent = t.segments_sent_persec as f64;
            let re = t.segments_retransmitted_persec as f64;
            (re, if sent > 0.0 { re / sent * 100.0 } else { 0.0 })
        })
        .unwrap_or((0.0, 0.0));
    let mut adapters: Vec<AdapterUtil> = hub
        .query::<NetInterface>()
        .map(|rows| {
            rows.into_iter()
                .filter(|a| {
                    a.current_bandwidth > 0
                        && !a.name.contains("Loopback")
                        && !a.name.contains("isatap")
                        && !a.name.contains("Npcap")
                })
                .map(|a| AdapterUtil {
                    util_pct: a.bytes_total_persec as f64 * 8.0 / a.current_bandwidth as f64
                        * 100.0,
                    name: a.name,
                    link_bps: a.current_bandwidth,
                })
                .collect()
        })
        .unwrap_or_default();
    adapters.sort_by(|a, b| {
        b.util_pct
            .partial_cmp(&a.util_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    adapters.truncate(4);
    NetExt {
        tcp_established,
        tcp_time_wait,
        tcp_listen,
        udp_endpoints,
        retrans_ps,
        retrans_pct,
        adapters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_tables_are_readable() {
        let (estab, tw, listen) = tcp_state_counts();
        let udp = udp_endpoint_count();
        println!("tcp estab={estab} time_wait={tw} listen={listen}, udp endpoints={udp}");
        // 桌面系统必然有监听端口与 UDP 端点
        assert!(listen > 0, "expected some listening TCP ports");
        assert!(udp > 0, "expected some UDP endpoints");
    }

    #[test]
    fn net_ext_samples_adapters() {
        let hub = WmiHub::new();
        let n = sample(&hub);
        println!(
            "adapters: {:?}",
            n.adapters
                .iter()
                .map(|a| format!("{} {}bps {:.1}%", a.name, a.link_bps, a.util_pct))
                .collect::<Vec<_>>()
        );
        for a in &n.adapters {
            assert!(a.link_bps > 0);
            assert!(a.util_pct >= 0.0);
        }
    }
}
