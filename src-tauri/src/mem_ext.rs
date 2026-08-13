use crate::pdh::{PdhCounter, PdhQuery};
use crate::wmi_hub::WmiHub;
use serde::{Deserialize, Serialize};

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
#[serde(rename = "Win32_PhysicalMemory")]
#[serde(rename_all = "PascalCase")]
struct PhysicalMemory {
    configured_clock_speed: Option<u32>,
    speed: Option<u32>,
}

/// 内存深化采集：提交/页错误/缓存页列表（PDH 动态）+ 内存硬件（WMI 一次性静态）
pub struct MemExtSampler {
    mem_speed_mts: u32,
    mem_modules: u32,
    c_commit: Option<PdhCounter>,
    c_limit: Option<PdhCounter>,
    c_pages_in: Option<PdhCounter>,
    c_faults: Option<PdhCounter>,
    c_standby_core: Option<PdhCounter>,
    c_standby_norm: Option<PdhCounter>,
    c_standby_res: Option<PdhCounter>,
    c_modified: Option<PdhCounter>,
}

impl MemExtSampler {
    pub fn new(hub: &WmiHub, pdh: &PdhQuery) -> Self {
        let (mem_speed_mts, mem_modules) = hub
            .query::<PhysicalMemory>()
            .map(|rows| {
                let speed = rows
                    .iter()
                    .filter_map(|m| m.configured_clock_speed.or(m.speed))
                    .max()
                    .unwrap_or(0);
                (speed, rows.len() as u32)
            })
            .unwrap_or((0, 0));
        let m = |c: &str| pdh.add(&format!("\\Memory\\{c}"));
        MemExtSampler {
            mem_speed_mts,
            mem_modules,
            c_commit: m("Committed Bytes"),
            c_limit: m("Commit Limit"),
            c_pages_in: m("Pages Input/sec"),
            c_faults: m("Page Faults/sec"),
            c_standby_core: m("Standby Cache Core Bytes"),
            c_standby_norm: m("Standby Cache Normal Priority Bytes"),
            c_standby_res: m("Standby Cache Reserve Bytes"),
            c_modified: m("Modified Page List Bytes"),
        }
    }

    pub fn sample(&self) -> MemExt {
        // 双通道理论带宽：MT/s × 64bit × 2 通道 ÷ 8（多于 2 条仍按双通道估算）
        let channels = self.mem_modules.clamp(1, 2) as f64;
        let theo = self.mem_speed_mts as f64 * 8.0 * channels / 1000.0;
        let v = |c: &Option<PdhCounter>| c.as_ref().and_then(|c| c.value()).unwrap_or(0.0);
        MemExt {
            commit_used: v(&self.c_commit) as u64,
            commit_limit: v(&self.c_limit) as u64,
            hard_faults_ps: v(&self.c_pages_in),
            page_faults_ps: v(&self.c_faults),
            standby_bytes: (v(&self.c_standby_core)
                + v(&self.c_standby_norm)
                + v(&self.c_standby_res)) as u64,
            modified_bytes: v(&self.c_modified) as u64,
            mem_speed_mts: self.mem_speed_mts,
            mem_modules: self.mem_modules,
            theo_bandwidth_gbps: theo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_ext_is_plausible() {
        let hub = WmiHub::new();
        let pdh = PdhQuery::new().unwrap();
        let s = MemExtSampler::new(&hub, &pdh);
        pdh.collect();
        std::thread::sleep(std::time::Duration::from_millis(400));
        pdh.collect();
        let m = s.sample();
        println!(
            "commit {}/{} GB, hard faults {}/s, cached {} GB, {} MT/s x {}",
            m.commit_used >> 30,
            m.commit_limit >> 30,
            m.hard_faults_ps,
            (m.standby_bytes + m.modified_bytes) >> 30,
            m.mem_speed_mts,
            m.mem_modules,
        );
        assert!(m.commit_limit > 0, "commit limit should be positive");
        assert!(m.commit_used > 0 && m.commit_used <= m.commit_limit);
        assert!(m.standby_bytes > 0, "standby list should not be empty");
    }
}
