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

/// 内存深化采集：提交/页错误/缓存页列表（动态）+ 内存硬件（静态缓存）
pub struct MemExtSampler {
    mem_speed_mts: u32,
    mem_modules: u32,
}

impl MemExtSampler {
    pub fn new(hub: &WmiHub) -> Self {
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
        MemExtSampler {
            mem_speed_mts,
            mem_modules,
        }
    }

    /// WMI 不可用时返回默认零值
    pub fn sample(&self, hub: &WmiHub) -> MemExt {
        // 双通道理论带宽：MT/s × 64bit × 2 通道 ÷ 8（多于 2 条仍按双通道估算）
        let channels = self.mem_modules.clamp(1, 2) as f64;
        let theo = self.mem_speed_mts as f64 * 8.0 * channels / 1000.0;
        hub.query::<PerfMemory>()
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
    fn mem_ext_is_plausible() {
        let hub = WmiHub::new();
        let s = MemExtSampler::new(&hub);
        let m = s.sample(&hub);
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
