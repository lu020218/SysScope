use crate::wmi_hub::WmiHub;
use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

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

/// CPU 性能计数采集：基准频率（静态）+ 有效频率比例与 C-State
pub struct CpuPerfSampler {
    base_mhz: u32,
}

impl CpuPerfSampler {
    pub fn new(hub: &WmiHub) -> Self {
        let base_mhz = hub
            .query::<Win32Processor>()
            .and_then(|rows| rows.into_iter().next())
            .map(|p| p.max_clock_speed)
            .unwrap_or(0);
        CpuPerfSampler { base_mhz }
    }

    /// CPU 基准频率（MHz，Win32_Processor.MaxClockSpeed），不可用为 0
    pub fn base_mhz(&self) -> u32 {
        self.base_mhz
    }

    /// 处理器性能计数（_Total 实例）：有效频率比例与 C-State 驻留
    pub fn sample(&self, hub: &WmiHub) -> Option<CpuPerf> {
        hub.query::<ProcessorInformation>()?
            .into_iter()
            .find(|r| r.name == "_Total")
            .map(|r| CpuPerf {
                perf_pct: r.percent_processor_performance as f64,
                c1_pct: r.percent_c1_time as f64,
                c2_pct: r.percent_c2_time as f64,
                c3_pct: r.percent_c3_time as f64,
            })
    }
}

/// 每个逻辑处理器的效率等级（Intel 混合架构：P 核等级高于 E 核；
/// 非混合架构全为同一等级）。失败时返回空数组。
pub fn core_efficiency_classes() -> Vec<u8> {
    unsafe {
        let mut len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut len,
        )
        .is_err()
        {
            return Vec::new();
        }

        let mut classes = vec![0u8; 64];
        let mut max_idx = 0usize;
        let mut off = 0usize;
        while off + 8 <= len as usize {
            let rec = &*(buf.as_ptr().add(off) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            if rec.Size == 0 {
                break;
            }
            if rec.Relationship == RelationProcessorCore {
                let proc_rel = &rec.Anonymous.Processor;
                let eff = proc_rel.EfficiencyClass;
                // 单处理器组场景取首个组掩码（>64 线程的多组机器暂不细分）
                let mask = proc_rel.GroupMask[0].Mask;
                for bit in 0..64usize {
                    if mask & (1usize << bit) != 0 {
                        classes[bit] = eff;
                        max_idx = max_idx.max(bit);
                    }
                }
            }
            off += rec.Size as usize;
        }
        classes.truncate(max_idx + 1);
        classes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_all_logical_cores() {
        let classes = core_efficiency_classes();
        println!("efficiency classes: {classes:?}");
        assert!(!classes.is_empty());
        let sys = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage()),
        );
        assert_eq!(classes.len(), sys.cpus().len());
    }

    #[test]
    fn perf_counters_are_plausible() {
        let hub = WmiHub::new();
        let s = CpuPerfSampler::new(&hub);
        println!("base_mhz={}", s.base_mhz());
        assert!(s.base_mhz() > 500, "implausible base frequency");
        if let Some(p) = s.sample(&hub) {
            println!("perf={}% c1={} c2={} c3={}", p.perf_pct, p.c1_pct, p.c2_pct, p.c3_pct);
            assert!(p.perf_pct > 0.0);
        }
    }
}
