use crate::pdh::{PdhCounter, PdhQuery};
use std::collections::HashMap;

/// 从 "pid_1234_luid_..." 实例名提取 PID
fn parse_pid(instance: &str) -> Option<u32> {
    instance
        .strip_prefix("pid_")?
        .split('_')
        .next()?
        .parse()
        .ok()
}

/// 每进程 GPU 占用/显存（PDH per-PID 实例计数器，成本已足够低，逐拍采样）
pub struct GpuProcSampler {
    c_engine: Option<PdhCounter>,
    c_mem: Option<PdhCounter>,
    cache: HashMap<u32, (f32, u64)>,
}

impl GpuProcSampler {
    pub fn new(pdh: &PdhQuery) -> Self {
        GpuProcSampler {
            c_engine: pdh.add("\\GPU Engine(*)\\Utilization Percentage"),
            c_mem: pdh.add("\\GPU Process Memory(*)\\Dedicated Usage"),
            cache: HashMap::new(),
        }
    }

    /// pid -> (占用%, 显存字节)
    pub fn sample(&mut self) -> &HashMap<u32, (f32, u64)> {
        let mut map: HashMap<u32, (f32, u64)> = HashMap::new();
        if let Some(c) = &self.c_engine {
            for (name, v) in c.array() {
                if let Some(pid) = parse_pid(&name) {
                    map.entry(pid).or_default().0 += v as f32;
                }
            }
        }
        if let Some(c) = &self.c_mem {
            for (name, v) in c.array() {
                if let Some(pid) = parse_pid(&name) {
                    map.entry(pid).or_default().1 += v as u64;
                }
            }
        }
        for v in map.values_mut() {
            v.0 = v.0.min(100.0);
        }
        self.cache = map;
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
        let pdh = PdhQuery::new().unwrap();
        let mut s = GpuProcSampler::new(&pdh);
        pdh.collect();
        std::thread::sleep(std::time::Duration::from_millis(400));
        pdh.collect();
        let map = s.sample();
        println!("{} process(es) with gpu activity", map.len());
        for (pid, (util, vram)) in map.iter().take(5) {
            println!("  pid {pid}: {util}% vram={vram}");
            assert!(*util <= 100.0);
        }
    }
}
