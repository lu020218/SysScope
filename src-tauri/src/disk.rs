use crate::pdh::{PdhCounter, PdhQuery};
use serde::Serialize;
use std::collections::BTreeMap;
use sysinfo::Disks;

#[derive(Serialize, Clone, Default)]
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
    /// 平均响应时间（ms；周期内无 IO 为 None）
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

/// 磁盘 I/O 采集：PDH PhysicalDisk 计数器 + sysinfo 分区空间
pub struct DiskSampler {
    volumes: Disks,
    c_idle: Option<PdhCounter>,
    c_read_bps: Option<PdhCounter>,
    c_write_bps: Option<PdhCounter>,
    c_queue: Option<PdhCounter>,
    c_read_iops: Option<PdhCounter>,
    c_write_iops: Option<PdhCounter>,
    c_read_lat: Option<PdhCounter>,
    c_write_lat: Option<PdhCounter>,
}

impl DiskSampler {
    pub fn new(pdh: &PdhQuery) -> Self {
        let p = |c: &str| pdh.add(&format!("\\PhysicalDisk(*)\\{c}"));
        DiskSampler {
            volumes: Disks::new_with_refreshed_list(),
            c_idle: p("% Idle Time"),
            c_read_bps: p("Disk Read Bytes/sec"),
            c_write_bps: p("Disk Write Bytes/sec"),
            c_queue: p("Current Disk Queue Length"),
            c_read_iops: p("Disk Reads/sec"),
            c_write_iops: p("Disk Writes/sec"),
            c_read_lat: p("Avg. Disk sec/Read"),
            c_write_lat: p("Avg. Disk sec/Write"),
        }
    }

    pub fn sample(&mut self) -> StorageSnapshot {
        // 按实例名聚合各计数器数组（BTreeMap 保证盘序稳定）
        let mut map: BTreeMap<String, DiskIo> = BTreeMap::new();
        let mut fill = |c: &Option<PdhCounter>, set: &mut dyn FnMut(&mut DiskIo, f64)| {
            if let Some(c) = c {
                for (name, v) in c.array() {
                    if name == "_Total" {
                        continue;
                    }
                    let e = map.entry(name.clone()).or_insert_with(|| DiskIo {
                        name,
                        ..DiskIo::default()
                    });
                    set(e, v);
                }
            }
        };
        fill(&self.c_idle, &mut |e, v| {
            e.active_pct = (100.0 - v).clamp(0.0, 100.0) as f32;
        });
        fill(&self.c_read_bps, &mut |e, v| e.read_bps = v);
        fill(&self.c_write_bps, &mut |e, v| e.write_bps = v);
        fill(&self.c_queue, &mut |e, v| e.queue_len = v);
        fill(&self.c_read_iops, &mut |e, v| e.read_iops = v);
        fill(&self.c_write_iops, &mut |e, v| e.write_iops = v);
        // PDH 直接给平均延迟（秒）；周期内无 IO 时为 0，映射为 None
        fill(&self.c_read_lat, &mut |e, v| {
            e.read_ms = (v > 0.0).then_some(v * 1000.0);
        });
        fill(&self.c_write_lat, &mut |e, v| {
            e.write_ms = (v > 0.0).then_some(v * 1000.0);
        });

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

        StorageSnapshot {
            disks: map.into_values().collect(),
            volumes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_sampler_produces_plausible_values() {
        let pdh = PdhQuery::new().unwrap();
        let mut s = DiskSampler::new(&pdh);
        pdh.collect();
        std::thread::sleep(std::time::Duration::from_millis(600));
        pdh.collect();
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
    }
}
