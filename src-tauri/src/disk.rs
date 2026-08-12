use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use sysinfo::Disks;
use wmi::{COMLibrary, WMIConnection};

#[derive(Serialize, Clone)]
pub struct DiskIo {
    /// PDH 实例名，如 "0 C:"
    pub name: String,
    /// 磁盘活动时间百分比（100 - 空闲）
    pub active_pct: f32,
    pub read_bps: f64,
    pub write_bps: f64,
    pub queue_len: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    /// 平均响应时间（ms，RawData 差分计算；无 IO 的周期为 None）
    pub read_ms: Option<f64>,
    pub write_ms: Option<f64>,
}

#[derive(Serialize, Clone)]
pub struct VolumeInfo {
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Serialize, Clone, Default)]
pub struct StorageSnapshot {
    pub disks: Vec<DiskIo>,
    pub volumes: Vec<VolumeInfo>,
}

#[derive(Serialize, Clone, Default)]
pub struct MemExt {
    /// 提交内存（字节）
    pub commit_used: u64,
    pub commit_limit: u64,
    /// 硬页面错误（Pages Input/sec，需从磁盘读页的缺页）
    pub hard_faults_ps: f64,
    /// 总页错误（含软错误）
    pub page_faults_ps: f64,
    /// 备用列表（三档合计）与已修改页列表（字节）；已缓存 = 两者之和
    pub standby_bytes: u64,
    pub modified_bytes: u64,
    /// 内存硬件（静态）：频率 MT/s、条数、双通道理论带宽 GB/s（推算值）
    pub mem_speed_mts: u32,
    pub mem_modules: u32,
    pub theo_bandwidth_gbps: f64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_PerfOS_Memory")]
#[serde(rename_all = "PascalCase")]
struct PerfMemory {
    committed_bytes: u64,
    commit_limit: u64,
    pages_input_persec: u64,
    page_faults_persec: u64,
    standby_cache_core_bytes: u64,
    standby_cache_normal_priority_bytes: u64,
    standby_cache_reserve_bytes: u64,
    modified_page_list_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PhysicalMemory")]
#[serde(rename_all = "PascalCase")]
struct PhysicalMemory {
    configured_clock_speed: Option<u32>,
    speed: Option<u32>,
}

#[derive(Serialize, Clone, Default)]
pub struct CpuPerf {
    /// % Processor Performance（任务管理器口径，>100 表示睿频）
    pub perf_pct: f64,
    /// C-State 驻留占比
    pub c1_pct: f64,
    pub c2_pct: f64,
    pub c3_pct: f64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_Counters_ProcessorInformation")]
#[serde(rename_all = "PascalCase")]
struct ProcessorInformation {
    name: String,
    percent_processor_performance: u64,
    percent_c1_time: u64,
    percent_c2_time: u64,
    percent_c3_time: u64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_Processor")]
#[serde(rename_all = "PascalCase")]
struct Win32Processor {
    max_clock_speed: u32,
}

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
#[serde(rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine")]
#[serde(rename_all = "PascalCase")]
struct GpuEnginePid {
    name: String,
    utilization_percentage: u64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory")]
#[serde(rename_all = "PascalCase")]
struct GpuProcessMemory {
    name: String,
    dedicated_usage: u64,
}

/// 从 "pid_1234_luid_..." 实例名提取 PID
fn parse_pid(instance: &str) -> Option<u32> {
    instance
        .strip_prefix("pid_")?
        .split('_')
        .next()?
        .parse()
        .ok()
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

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_PerfDisk_PhysicalDisk")]
#[serde(rename_all = "PascalCase")]
struct PhysicalDisk {
    name: String,
    percent_idle_time: u64,
    disk_read_bytes_persec: u64,
    disk_write_bytes_persec: u64,
    current_disk_queue_length: u32,
    disk_reads_persec: u32,
    disk_writes_persec: u32,
}

/// 响应时间必须用原始计数器差分：格式化版 AvgDisksecPerRead 是整数秒，
/// 亚毫秒级延迟会被截断为 0
#[derive(Deserialize)]
#[serde(rename = "Win32_PerfRawData_PerfDisk_PhysicalDisk")]
#[serde(rename_all = "PascalCase")]
struct RawDisk {
    name: String,
    avg_disksec_per_read: u64,
    #[serde(rename = "AvgDisksecPerRead_Base")]
    avg_disksec_per_read_base: u32,
    avg_disksec_per_write: u64,
    #[serde(rename = "AvgDisksecPerWrite_Base")]
    avg_disksec_per_write_base: u32,
    #[serde(rename = "Frequency_PerfTime")]
    frequency_perf_time: u64,
}

#[derive(Default, Clone, Copy)]
struct RawLatencySample {
    read_ticks: u64,
    read_ops: u32,
    write_ticks: u64,
    write_ops: u32,
}

/// 磁盘 I/O 采集：WMI 格式化性能计数器 + sysinfo 分区空间。
/// 必须在采样线程内创建（依赖线程 COM 环境）。
pub struct DiskSampler {
    conn: Option<WMIConnection>,
    volumes: Disks,
    base_mhz: u32,
    mem_speed_mts: u32,
    mem_modules: u32,
    /// 上一轮原始延迟计数（按实例名），用于差分
    prev_latency: HashMap<String, RawLatencySample>,
    /// 每进程 GPU 占用/显存缓存（实例多、查询较重，2 秒采一次）
    gpu_proc_cache: HashMap<u32, (f32, u64)>,
    gpu_proc_at: Option<Instant>,
}

impl DiskSampler {
    pub fn new() -> Self {
        let conn = COMLibrary::new()
            .or_else(|_| Ok::<_, wmi::WMIError>(unsafe { COMLibrary::assume_initialized() }))
            .ok()
            .and_then(|com| WMIConnection::new(com).ok());
        if conn.is_none() {
            eprintln!("[sysscope] disk sampler: WMI unavailable");
        }
        let base_mhz = conn
            .as_ref()
            .and_then(|c| c.query::<Win32Processor>().ok())
            .and_then(|rows| rows.into_iter().next())
            .map(|p| p.max_clock_speed)
            .unwrap_or(0);
        let (mem_speed_mts, mem_modules) = conn
            .as_ref()
            .and_then(|c| c.query::<PhysicalMemory>().ok())
            .map(|rows| {
                let speed = rows
                    .iter()
                    .filter_map(|m| m.configured_clock_speed.or(m.speed))
                    .max()
                    .unwrap_or(0);
                (speed, rows.len() as u32)
            })
            .unwrap_or((0, 0));
        DiskSampler {
            conn,
            volumes: Disks::new_with_refreshed_list(),
            base_mhz,
            mem_speed_mts,
            mem_modules,
            prev_latency: HashMap::new(),
            gpu_proc_cache: HashMap::new(),
            gpu_proc_at: None,
        }
    }

    /// 每进程 GPU：pid -> (占用%, 显存字节)，2 秒缓存
    pub fn sample_gpu_procs(&mut self) -> &HashMap<u32, (f32, u64)> {
        let fresh = self
            .gpu_proc_at
            .map(|t| t.elapsed().as_secs_f64() < 2.0)
            .unwrap_or(false);
        if !fresh {
            self.gpu_proc_at = Some(Instant::now());
            let mut map: HashMap<u32, (f32, u64)> = HashMap::new();
            if let Some(conn) = self.conn.as_ref() {
                if let Ok(rows) = conn.query::<GpuEnginePid>() {
                    for r in rows {
                        if let Some(pid) = parse_pid(&r.name) {
                            map.entry(pid).or_default().0 += r.utilization_percentage as f32;
                        }
                    }
                }
                if let Ok(rows) = conn.query::<GpuProcessMemory>() {
                    for r in rows {
                        if let Some(pid) = parse_pid(&r.name) {
                            map.entry(pid).or_default().1 += r.dedicated_usage;
                        }
                    }
                }
            }
            for v in map.values_mut() {
                v.0 = v.0.min(100.0);
            }
            self.gpu_proc_cache = map;
        }
        &self.gpu_proc_cache
    }

    /// 差分计算每盘读写平均响应时间（ms）
    fn sample_latency(&mut self) -> HashMap<String, (Option<f64>, Option<f64>)> {
        let mut out = HashMap::new();
        let Some(rows) = self
            .conn
            .as_ref()
            .and_then(|c| c.query::<RawDisk>().ok())
        else {
            return out;
        };
        for r in rows {
            if r.name == "_Total" {
                continue;
            }
            let cur = RawLatencySample {
                read_ticks: r.avg_disksec_per_read,
                read_ops: r.avg_disksec_per_read_base,
                write_ticks: r.avg_disksec_per_write,
                write_ops: r.avg_disksec_per_write_base,
            };
            let freq = r.frequency_perf_time.max(1) as f64;
            if let Some(prev) = self.prev_latency.get(&r.name) {
                let lat = |ticks: u64, pticks: u64, ops: u32, pops: u32| -> Option<f64> {
                    let dops = ops.wrapping_sub(pops);
                    if dops == 0 {
                        return None;
                    }
                    let dticks = ticks.wrapping_sub(pticks) as f64;
                    Some(dticks / freq / dops as f64 * 1000.0)
                };
                out.insert(
                    r.name.clone(),
                    (
                        lat(cur.read_ticks, prev.read_ticks, cur.read_ops, prev.read_ops),
                        lat(cur.write_ticks, prev.write_ticks, cur.write_ops, prev.write_ops),
                    ),
                );
            }
            self.prev_latency.insert(r.name, cur);
        }
        out
    }

    /// CPU 基准频率（MHz，Win32_Processor.MaxClockSpeed），不可用为 0
    pub fn base_mhz(&self) -> u32 {
        self.base_mhz
    }

    /// 网络深化：TCP/UDP 连接统计、重传率、各网卡链路利用率
    pub fn sample_net_ext(&self) -> NetExt {
        let (tcp_established, tcp_time_wait, tcp_listen) = crate::netstat::tcp_state_counts();
        let udp_endpoints = crate::netstat::udp_endpoint_count();
        let (retrans_ps, retrans_pct) = self
            .conn
            .as_ref()
            .and_then(|c| c.query::<TcpV4>().ok())
            .and_then(|rows| rows.into_iter().next())
            .map(|t| {
                let sent = t.segments_sent_persec as f64;
                let re = t.segments_retransmitted_persec as f64;
                (re, if sent > 0.0 { re / sent * 100.0 } else { 0.0 })
            })
            .unwrap_or((0.0, 0.0));
        let mut adapters: Vec<AdapterUtil> = self
            .conn
            .as_ref()
            .and_then(|c| c.query::<NetInterface>().ok())
            .map(|rows| {
                rows.into_iter()
                    .filter(|a| {
                        a.current_bandwidth > 0
                            && !a.name.contains("Loopback")
                            && !a.name.contains("isatap")
                            && !a.name.contains("Npcap")
                    })
                    .map(|a| AdapterUtil {
                        util_pct: a.bytes_total_persec as f64 * 8.0
                            / a.current_bandwidth as f64
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

    /// 处理器性能计数（_Total 实例）：有效频率比例与 C-State 驻留
    pub fn sample_cpu_perf(&self) -> Option<CpuPerf> {
        let rows = self
            .conn
            .as_ref()
            .and_then(|c| c.query::<ProcessorInformation>().ok())?;
        rows.into_iter()
            .find(|r| r.name == "_Total")
            .map(|r| CpuPerf {
                perf_pct: r.percent_processor_performance as f64,
                c1_pct: r.percent_c1_time as f64,
                c2_pct: r.percent_c2_time as f64,
                c3_pct: r.percent_c3_time as f64,
            })
    }

    pub fn sample(&mut self) -> StorageSnapshot {
        let latency = self.sample_latency();
        let disks = self
            .conn
            .as_ref()
            .and_then(|c| c.query::<PhysicalDisk>().ok())
            .map(|rows| {
                rows.into_iter()
                    .filter(|d| d.name != "_Total")
                    .map(|d| {
                        let (read_ms, write_ms) =
                            latency.get(&d.name).copied().unwrap_or((None, None));
                        DiskIo {
                            active_pct: (100u64.saturating_sub(d.percent_idle_time)) as f32,
                            read_bps: d.disk_read_bytes_persec as f64,
                            write_bps: d.disk_write_bytes_persec as f64,
                            queue_len: d.current_disk_queue_length as f64,
                            read_iops: d.disk_reads_persec as f64,
                            write_iops: d.disk_writes_persec as f64,
                            read_ms,
                            write_ms,
                            name: d.name,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        self.volumes.refresh(true);
        let mut volumes: Vec<VolumeInfo> = self
            .volumes
            .iter()
            .map(|d| VolumeInfo {
                mount: d.mount_point().to_string_lossy().into_owned(),
                total: d.total_space(),
                available: d.available_space(),
            })
            .collect();
        volumes.sort_by(|a, b| a.mount.cmp(&b.mount));
        volumes.dedup_by(|a, b| a.mount == b.mount);

        StorageSnapshot { disks, volumes }
    }

    /// 提交内存、页错误、缓存页列表与内存硬件信息（WMI 不可用时返回默认零值）
    pub fn sample_mem_ext(&self) -> MemExt {
        // 双通道理论带宽：MT/s × 64bit × 2 通道 ÷ 8（多于 2 条仍按双通道估算）
        let channels = self.mem_modules.clamp(1, 2) as f64;
        let theo = self.mem_speed_mts as f64 * 8.0 * channels / 1000.0;
        self.conn
            .as_ref()
            .and_then(|c| c.query::<PerfMemory>().ok())
            .and_then(|rows| rows.into_iter().next())
            .map(|m| MemExt {
                commit_used: m.committed_bytes,
                commit_limit: m.commit_limit,
                hard_faults_ps: m.pages_input_persec as f64,
                page_faults_ps: m.page_faults_persec as f64,
                standby_bytes: m.standby_cache_core_bytes
                    + m.standby_cache_normal_priority_bytes
                    + m.standby_cache_reserve_bytes,
                modified_bytes: m.modified_page_list_bytes,
                mem_speed_mts: self.mem_speed_mts,
                mem_modules: self.mem_modules,
                theo_bandwidth_gbps: theo,
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_sampler_produces_plausible_values() {
        let mut s = DiskSampler::new();
        // 第一次查询计数器可能全为 0（延迟差分也需要基线），取第二次
        let _ = s.sample();
        std::thread::sleep(std::time::Duration::from_millis(600));
        let snap = s.sample();
        println!(
            "disks: {:?}",
            snap.disks
                .iter()
                .map(|d| format!(
                    "{} act={}% r={:.0} w={:.0} iops={:.0}/{:.0} lat={:?}/{:?}ms q={}",
                    d.name,
                    d.active_pct,
                    d.read_bps,
                    d.write_bps,
                    d.read_iops,
                    d.write_iops,
                    d.read_ms.map(|v| (v * 100.0).round() / 100.0),
                    d.write_ms.map(|v| (v * 100.0).round() / 100.0),
                    d.queue_len,
                ))
                .collect::<Vec<_>>()
        );
        println!(
            "volumes: {:?}",
            snap.volumes
                .iter()
                .map(|v| format!("{} {}/{}GB", v.mount, v.available >> 30, v.total >> 30))
                .collect::<Vec<_>>()
        );
        assert!(!snap.disks.is_empty(), "no physical disks found");
        assert!(!snap.volumes.is_empty(), "no volumes found");
        for d in &snap.disks {
            assert!((0.0..=100.0).contains(&d.active_pct));
            assert!(d.name != "_Total");
        }
        for v in &snap.volumes {
            assert!(v.total > 0 && v.available <= v.total);
        }

        let m = s.sample_mem_ext();
        println!(
            "commit {}/{} GB, hard faults {}/s",
            m.commit_used >> 30,
            m.commit_limit >> 30,
            m.hard_faults_ps
        );
        assert!(m.commit_limit > 0, "commit limit should be positive");
        assert!(m.commit_used > 0 && m.commit_used <= m.commit_limit);
    }
}
