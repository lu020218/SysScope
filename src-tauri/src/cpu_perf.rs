use crate::pdh::{PdhCounter, PdhQuery};
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
#[serde(rename = "Win32_Processor")]
#[serde(rename_all = "PascalCase")]
struct Win32Processor {
    max_clock_speed: u32,
}

/// CPU 性能计数采集：基准频率（WMI 一次性静态）+ PDH 有效频率比例与 C-State
pub struct CpuPerfSampler {
    base_mhz: u32,
    c_perf: Option<PdhCounter>,
    c_c1: Option<PdhCounter>,
    c_c2: Option<PdhCounter>,
    c_c3: Option<PdhCounter>,
}

impl CpuPerfSampler {
    pub fn new(hub: &WmiHub, pdh: &PdhQuery) -> Self {
        let base_mhz = hub
            .query::<Win32Processor>()
            .and_then(|rows| rows.into_iter().next())
            .map(|p| p.max_clock_speed)
            .unwrap_or(0);
        let p = |c: &str| pdh.add(&format!("\\Processor Information(_Total)\\{c}"));
        CpuPerfSampler {
            base_mhz,
            c_perf: p("% Processor Performance"),
            c_c1: p("% C1 Time"),
            c_c2: p("% C2 Time"),
            c_c3: p("% C3 Time"),
        }
    }

    /// CPU 基准频率（MHz，Win32_Processor.MaxClockSpeed），不可用为 0
    pub fn base_mhz(&self) -> u32 {
        self.base_mhz
    }

    pub fn sample(&self) -> Option<CpuPerf> {
        let perf = self.c_perf.as_ref()?.value()?;
        let v = |c: &Option<PdhCounter>| c.as_ref().and_then(|c| c.value()).unwrap_or(0.0);
        Some(CpuPerf {
            perf_pct: perf,
            c1_pct: v(&self.c_c1),
            c2_pct: v(&self.c_c2),
            c3_pct: v(&self.c_c3),
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
                for (bit, class) in classes.iter_mut().enumerate().take(64) {
                    if mask & (1usize << bit) != 0 {
                        *class = eff;
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
        let pdh = PdhQuery::new().unwrap();
        let s = CpuPerfSampler::new(&hub, &pdh);
        println!("base_mhz={}", s.base_mhz());
        assert!(s.base_mhz() > 500, "implausible base frequency");
        pdh.collect();
        std::thread::sleep(std::time::Duration::from_millis(400));
        pdh.collect();
        if let Some(p) = s.sample() {
            println!("perf={}% c1={} c2={} c3={}", p.perf_pct, p.c1_pct, p.c2_pct, p.c3_pct);
            assert!(p.perf_pct > 0.0);
        }
    }
}
