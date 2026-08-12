use crate::wmi_hub::WmiHub;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use sysinfo::Disks;

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

/// 磁盘 I/O 采集：WMI 性能计数器 + sysinfo 分区空间
pub struct DiskSampler {
    volumes: Disks,
    /// 上一轮原始延迟计数（按实例名），用于差分
    prev_latency: HashMap<String, RawLatencySample>,
}

impl DiskSampler {
    pub fn new() -> Self {
        DiskSampler {
            volumes: Disks::new_with_refreshed_list(),
            prev_latency: HashMap::new(),
        }
    }

    /// 差分计算每盘读写平均响应时间（ms）
    fn sample_latency(&mut self, hub: &WmiHub) -> HashMap<String, (Option<f64>, Option<f64>)> {
        let mut out = HashMap::new();
        let Some(rows) = hub.query::<RawDisk>() else {
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

    pub fn sample(&mut self, hub: &WmiHub) -> StorageSnapshot {
        let latency = self.sample_latency(hub);
        let disks = hub
            .query::<PhysicalDisk>()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_sampler_produces_plausible_values() {
        let hub = WmiHub::new();
        let mut s = DiskSampler::new();
        // 第一次查询计数器可能全为 0（延迟差分也需要基线），取第二次
        let _ = s.sample(&hub);
        std::thread::sleep(std::time::Duration::from_millis(600));
        let snap = s.sample(&hub);
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
    }
}
