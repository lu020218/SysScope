use libloading::Library;
use std::path::PathBuf;

/// LibreHardwareMonitor 传感器桥接（sysscope_sensors.dll，NativeAOT 原生库）。
/// DLL 缺失或初始化失败（如无管理员权限导致驱动加载失败）时返回 None，
/// 上层将温度显示为不可用，不影响其他指标。
#[derive(serde::Deserialize, Default, Clone)]
pub struct SensorData {
    pub cpu_temp: Option<f32>,
    /// CPU 包功耗（W）
    pub cpu_power: Option<f32>,
    /// CPU 核心电压（V）
    pub cpu_voltage: Option<f32>,
    /// 每物理核心当前频率（MHz，LHM 枚举顺序）
    #[serde(default)]
    pub core_clocks: Vec<f32>,
    /// 主 GPU 的 NVAPI 侧传感器（NVML 不提供）
    pub gpu_hotspot: Option<f32>,
    pub gpu_fan_rpm: Option<f32>,
    pub gpu_vram_temp: Option<f32>,
    #[serde(default)]
    pub storage: Vec<StorageTemp>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct StorageTemp {
    pub name: String,
    /// 复合温度 / 控制器温度（℃）
    pub temp: Option<f32>,
    pub temp2: Option<f32>,
    /// SMART 剩余寿命（%）
    pub life: Option<f32>,
    /// 累计写入量（GB，TBW 统计）
    pub written_gb: Option<f32>,
}

pub struct SensorBridge {
    /// 保持库加载，函数指针的生命周期依赖它
    _lib: Library,
    sensors_json: unsafe extern "C" fn(*mut u8, i32) -> i32,
    shutdown: unsafe extern "C" fn(),
}

impl SensorBridge {
    pub fn init() -> Option<Self> {
        let path = locate_dll()?;
        unsafe {
            let lib = Library::new(&path).ok()?;
            let init = *lib
                .get::<unsafe extern "C" fn() -> i32>(b"sysscope_sensors_init")
                .ok()?;
            if init() != 0 {
                eprintln!(
                    "[sysscope] sensor bridge init failed: {}",
                    last_error(&lib)
                );
                return None;
            }
            let sensors_json = *lib
                .get::<unsafe extern "C" fn(*mut u8, i32) -> i32>(b"sysscope_sensors_json")
                .ok()?;
            let shutdown = *lib
                .get::<unsafe extern "C" fn()>(b"sysscope_sensors_shutdown")
                .ok()?;
            println!("[sysscope] sensor bridge loaded: {}", path.display());
            Some(SensorBridge {
                _lib: lib,
                sensors_json,
                shutdown,
            })
        }
    }

    pub fn read(&self) -> SensorData {
        let mut buf = vec![0u8; 16384];
        let n = unsafe { (self.sensors_json)(buf.as_mut_ptr(), buf.len() as i32) };
        if n <= 0 {
            return SensorData::default();
        }
        serde_json::from_slice(&buf[..n as usize]).unwrap_or_default()
    }
}

impl Drop for SensorBridge {
    fn drop(&mut self) {
        unsafe { (self.shutdown)() }
    }
}

fn last_error(lib: &Library) -> String {
    unsafe {
        let Ok(f) =
            lib.get::<unsafe extern "C" fn(*mut u8, i32) -> i32>(b"sysscope_last_error")
        else {
            return "unknown".into();
        };
        let mut buf = vec![0u8; 4096];
        let n = f(buf.as_mut_ptr(), buf.len() as i32).clamp(0, buf.len() as i32) as usize;
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

fn locate_dll() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("sysscope_sensors.dll"));
            candidates.push(dir.join("resources").join("sysscope_sensors.dll"));
        }
    }
    if cfg!(debug_assertions) {
        // 开发模式：直接从源码树 resources 目录加载
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("sysscope_sensors.dll"),
        );
    }
    candidates.into_iter().find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 需要管理员权限（内核驱动）；无权限或 DLL 缺失时跳过断言
    #[test]
    #[ignore = "hw: 需要管理员加载 LHM 内核驱动"]
    fn sensor_bridge_reads_cpu_temp() {
        let Some(bridge) = SensorBridge::init() else {
            println!("sensor bridge unavailable, skipping");
            return;
        };
        let data = bridge.read();
        println!(
            "cpu temp: {:?}, power: {:?}, storage: {:?}",
            data.cpu_temp,
            data.cpu_power,
            data.storage
                .iter()
                .map(|s| {
                    format!(
                        "{} temp={:?} life={:?} written={:?}GB",
                        s.name, s.temp, s.life, s.written_gb
                    )
                })
                .collect::<Vec<_>>()
        );
        let t = data.cpu_temp.expect("bridge loaded but no temperature");
        assert!((10.0..=110.0).contains(&t), "implausible cpu temp: {t}");
        for s in &data.storage {
            if let Some(temp) = s.temp {
                assert!((0.0..=100.0).contains(&temp), "implausible disk temp");
            }
            if let Some(life) = s.life {
                assert!((0.0..=100.0).contains(&life), "implausible ssd life");
            }
        }
    }
}
