use crate::wmi_hub::WmiHub;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Instant;

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

/// 每进程 GPU 占用/显存：实例多、查询较重，2 秒采一次并缓存
pub struct GpuProcSampler {
    cache: HashMap<u32, (f32, u64)>,
    sampled_at: Option<Instant>,
}

impl GpuProcSampler {
    pub fn new() -> Self {
        GpuProcSampler {
            cache: HashMap::new(),
            sampled_at: None,
        }
    }

    /// pid -> (占用%, 显存字节)
    pub fn sample(&mut self, hub: &WmiHub) -> &HashMap<u32, (f32, u64)> {
        let fresh = self
            .sampled_at
            .map(|t| t.elapsed().as_secs_f64() < 2.0)
            .unwrap_or(false);
        if !fresh {
            self.sampled_at = Some(Instant::now());
            let mut map: HashMap<u32, (f32, u64)> = HashMap::new();
            if let Some(rows) = hub.query::<GpuEnginePid>() {
                for r in rows {
                    if let Some(pid) = parse_pid(&r.name) {
                        map.entry(pid).or_default().0 += r.utilization_percentage as f32;
                    }
                }
            }
            if let Some(rows) = hub.query::<GpuProcessMemory>() {
                for r in rows {
                    if let Some(pid) = parse_pid(&r.name) {
                        map.entry(pid).or_default().1 += r.dedicated_usage;
                    }
                }
            }
            for v in map.values_mut() {
                v.0 = v.0.min(100.0);
            }
            self.cache = map;
        }
        &self.cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pid_instances() {
        assert_eq!(parse_pid("pid_1234_luid_0x0_0x1_phys_0_engtype_3D"), Some(1234));
        assert_eq!(parse_pid("luid_only"), None);
    }

    #[test]
    fn per_process_gpu_returns_bounded_values() {
        let hub = WmiHub::new();
        let mut s = GpuProcSampler::new();
        let map = s.sample(&hub);
        println!("{} process(es) with gpu activity", map.len());
        for (pid, (util, vram)) in map.iter().take(5) {
            println!("  pid {pid}: {util}% vram={vram}");
            assert!(*util <= 100.0);
        }
    }
}
