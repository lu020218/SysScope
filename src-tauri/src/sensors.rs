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
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Default)]
pub struct BoardSensors {
    pub name: String,
    /// 已接风扇的转速；未接的接口报 0 转，桥接侧已过滤
    #[serde(default)]
    pub fans: Vec<NamedValue>,
    /// 主板温度点（VRM、芯片组、PCH 等，命名随主板而异）
    #[serde(default)]
    pub temps: Vec<NamedValue>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct NamedValue {
    pub name: String,
    #[serde(alias = "rpm", alias = "value")]
    pub value: f32,
}

#[cfg(test)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct DomainTiming {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub ms: f32,
    pub sensors: u32,
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
    /// 可选：旧版 DLL 没有这个导出。缺失时主板传感器不可用，
    /// 但其余传感器照常工作 —— 不因为一个新导出缺失就让整个桥失效
    board_json: Option<unsafe extern "C" fn(*mut u8, i32) -> i32>,
    /// 同上：旧版 DLL 无此导出时 SMART 不可用，其余传感器照常
    storage_json: Option<unsafe extern "C" fn(*mut u8, i32) -> i32>,
    /// 诊断专用：逐域 Update() 计时。仅测试构建持有 —— 它不参与生产路径，
    /// 用 cfg(test) 门控而非 allow(dead_code)，避免把"未使用"当成常态忽略
    #[cfg(test)]
    timing_json: Option<unsafe extern "C" fn(*mut u8, i32) -> i32>,
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
            let board_json = lib
                .get::<unsafe extern "C" fn(*mut u8, i32) -> i32>(b"sysscope_board_json")
                .ok()
                .map(|f| *f);
            let storage_json = lib
                .get::<unsafe extern "C" fn(*mut u8, i32) -> i32>(b"sysscope_storage_json")
                .ok()
                .map(|f| *f);
            #[cfg(test)]
            let timing_json = lib
                .get::<unsafe extern "C" fn(*mut u8, i32) -> i32>(b"sysscope_timing_json")
                .ok()
                .map(|f| *f);
            let shutdown = *lib
                .get::<unsafe extern "C" fn()>(b"sysscope_sensors_shutdown")
                .ok()?;
            println!("[sysscope] sensor bridge loaded: {}", path.display());
            Some(SensorBridge {
                _lib: lib,
                sensors_json,
                board_json,
                storage_json,
                #[cfg(test)]
                timing_json,
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

    /// 诊断：逐个硬件域的 Update() 耗时。定位"哪个域慢"用，不进生产路径。
    #[cfg(test)]
    pub fn read_timings(&self) -> Option<Vec<DomainTiming>> {
        let f = self.timing_json?;
        let mut buf = vec![0u8; 8192];
        let n = unsafe { f(buf.as_mut_ptr(), buf.len() as i32) };
        if n <= 0 {
            return None;
        }
        serde_json::from_slice(&buf[..n as usize]).ok()
    }

    /// 硬盘 SMART。与 read() 分开，且**绝不可每拍调用** ——
    /// LHM 的 Storage.Update() 走 SMART IOCTL，实测两块 NVMe 合计约 390ms
    /// 中位数，曾占满整拍的 95%（而当时文档还写着"单拍约 20ms"）。
    /// SMART 数据变化以秒/月/天计，十秒级轮询绰绰有余。
    pub fn read_storage(&self) -> Option<Vec<StorageTemp>> {
        let f = self.storage_json?;
        let mut buf = vec![0u8; 8192];
        let n = unsafe { f(buf.as_mut_ptr(), buf.len() as i32) };
        if n <= 0 {
            return None;
        }
        serde_json::from_slice(&buf[..n as usize]).ok()
    }

    /// 主板 SuperIO 传感器。与 read() 分开低频调用。
    /// （注：最初以为 SuperIO 是慢的那个，实测只要 1.2ms —— 真正昂贵的是
    /// 上面的 SMART。拆分本身仍然合理，但当时的理由是错的。）
    pub fn read_board(&self) -> Option<BoardSensors> {
        let f = self.board_json?;
        let mut buf = vec![0u8; 8192];
        let n = unsafe { f(buf.as_mut_ptr(), buf.len() as i32) };
        if n <= 0 {
            return None;
        }
        serde_json::from_slice(&buf[..n as usize]).ok()
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
    // 开发与测试：回退到源码树的 resources 目录。
    // 此前这段限定在 debug_assertions 下，导致 `cargo test --release` 里桥
    // 静默加载失败 —— 性能分解测试因此报出"sensors.read 0.0ms"，看起来像
    // 传感器不花时间，实际是根本没调用。发布产物有自己的 resources 目录，
    // 走不到这条回退，因此放开它不影响发布行为。
    #[cfg(any(debug_assertions, test))]
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("sysscope_sensors.dll"),
    );
    candidates.into_iter().find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手动诊断：打印本机主板传感器与单次读取耗时。
    /// 主板域读的是 SuperIO 芯片，走 LPC/EC 端口 I/O，比其余传感器慢得多，
    /// 而它每拍都会被调用 —— 接入前必须先量清楚代价。
    ///   cargo test --lib board_sensors_dump -- --ignored --nocapture
    #[test]
    #[ignore = "hw: 手动诊断，打印主板传感器与耗时"]
    fn board_sensors_dump() {
        // init 失败的真实原因由 sensors.rs 的 eprintln 打印（可能是缺管理员
        // 权限，也可能是 NativeAOT 封送元数据缺失导致 Computer.Open() 抛异常
        // —— 后者会让整个桥失效，CPU 温度与 SMART 一并丢失，务必看清上面那行）
        let Some(bridge) = SensorBridge::init() else {
            println!("sensor bridge init returned None -- see the error printed above");
            return;
        };
        // 首次读取含驱动预热，量第二次起
        let _ = bridge.read();
        let _ = bridge.read_board();
        // 报分布而非最大值：LHM 会周期性重刷 SMART，偶发的慢拍会把最大值
        // 拉到与典型值差两个数量级，只看 max 会得出完全错误的结论
        let mut all = Vec::new();
        let mut brd = Vec::new();
        let mut data = SensorData::default();
        let mut board = None;
        for _ in 0..20 {
            let t0 = std::time::Instant::now();
            data = bridge.read();
            all.push(t0.elapsed());
            let t1 = std::time::Instant::now();
            board = bridge.read_board();
            brd.push(t1.elapsed());
        }
        let stat = |v: &mut Vec<std::time::Duration>| {
            v.sort();
            format!(
                "min {:?} / median {:?} / max {:?}",
                v[0],
                v[v.len() / 2],
                v[v.len() - 1]
            )
        };
        if let Some(mut t) = bridge.read_timings() {
            t.sort_by(|a, b| b.ms.partial_cmp(&a.ms).unwrap_or(std::cmp::Ordering::Equal));
            println!("=== 每域 Update() 耗时（降序）===");
            for d in &t {
                println!("  {:<10} {:>8.1} ms  {:>3} sensors  {}", d.kind, d.ms, d.sensors, d.name);
            }
        }
        match &board {
            Some(b) => {
                println!("board: {}", b.name);
                println!("  fans:");
                for f in &b.fans {
                    println!("    {:<28} {:.0} RPM", f.name, f.value);
                }
                println!("  temps:");
                for t in &b.temps {
                    println!("    {:<28} {:.1} C", t.name, t.value);
                }
            }
            None => println!("no motherboard sensors reported"),
        }
        // 两个耗时分开量：前者每拍都付，后者按 BOARD_REFRESH 秒级付一次
        println!(
            "cpu_temp: {:?}
  per-tick sensors.read(): {}
  board read (low freq): {}",
            data.cpu_temp,
            stat(&mut all),
            stat(&mut brd)
        );
    }

    /// 需要管理员权限（内核驱动）；无权限或 DLL 缺失时跳过断言
    #[test]
    #[ignore = "hw: 需要管理员加载 LHM 内核驱动"]
    fn sensor_bridge_reads_cpu_temp() {
        let Some(bridge) = SensorBridge::init() else {
            println!("sensor bridge unavailable, skipping");
            return;
        };
        let data = bridge.read();
        // SMART 已拆到独立的低频导出，不再随每拍读取返回
        let storage = bridge.read_storage().unwrap_or_default();
        println!(
            "cpu temp: {:?}, power: {:?}, storage: {:?}",
            data.cpu_temp,
            data.cpu_power,
            storage
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
        for s in &storage {
            if let Some(temp) = s.temp {
                assert!((0.0..=100.0).contains(&temp), "implausible disk temp");
            }
            if let Some(life) = s.life {
                assert!((0.0..=100.0).contains(&life), "implausible ssd life");
            }
        }
    }
}
