use nvml_wrapper::bitmasks::device::ThrottleReasons;
use nvml_wrapper::enum_wrappers::device::{
    Clock, PcieUtilCounter, TemperatureSensor, TemperatureThreshold,
};
use nvml_wrapper::Nvml;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wmi::{COMLibrary, WMIConnection};

#[derive(Serialize, Clone)]
pub struct GpuSnapshot {
    pub name: String,
    /// 占用率 0-100
    pub util_pct: f32,
    /// 显存，字节；WMI 兜底路径下 vram_total 可能为 0（未知）
    pub vram_used: u64,
    pub vram_total: u64,
    pub temp_c: Option<u32>,
    pub power_w: Option<f32>,
    /// 功耗上限（W），用于计算功耗墙占比
    pub power_limit_w: Option<f32>,
    /// 核心 / 显存当前频率（MHz）
    pub core_mhz: Option<u32>,
    pub mem_mhz: Option<u32>,
    /// 风扇转速百分比
    pub fan_pct: Option<u32>,
    /// 显存控制器负载 0-100
    pub mem_ctrl_pct: Option<u32>,
    /// 视频编码 / 解码引擎负载 0-100
    pub enc_pct: Option<u32>,
    pub dec_pct: Option<u32>,
    /// PCIe 吞吐（KB/s，低频采样缓存值）
    pub pcie_rx_kbs: Option<u32>,
    pub pcie_tx_kbs: Option<u32>,
    /// 硬件级节流标志（NVML throttle reasons）
    pub throttle_thermal: bool,
    pub throttle_power: bool,
    /// 降频阈值温度（℃）
    pub temp_slowdown_c: Option<u32>,
    /// 以下三项来自 LHM/NVAPI 桥（仅主 GPU）
    pub hotspot_c: Option<f32>,
    pub fan_rpm: Option<f32>,
    pub vram_temp_c: Option<f32>,
}

/// NVML 状态：PCIe 吞吐查询每次内部采样约 10-20ms，
/// 按 PCIE_SAMPLE_EVERY 个周期采一次并缓存
pub struct NvmlGpu {
    nvml: Nvml,
    tick: u32,
    pcie_cache: Option<(u32, u32)>,
}

const PCIE_SAMPLE_EVERY: u32 = 5;

/// GPU 采集后端：优先 NVML（NVIDIA），失败则回退 WMI GPU 性能计数器，
/// 两者皆不可用时降级为无 GPU 数据
pub enum GpuBackend {
    Nvml(Box<NvmlGpu>),
    Wmi(WmiGpu),
    None,
}

impl GpuBackend {
    /// 必须在采样线程内调用（WMI 依赖线程 COM 环境）
    pub fn init() -> Self {
        match Nvml::init() {
            Ok(nvml) if nvml.device_count().map(|c| c > 0).unwrap_or(false) => {
                GpuBackend::Nvml(Box::new(NvmlGpu {
                    nvml,
                    tick: 0,
                    pcie_cache: None,
                }))
            }
            _ => match WmiGpu::init() {
                Ok(w) => GpuBackend::Wmi(w),
                Err(_) => GpuBackend::None,
            },
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            GpuBackend::Nvml(_) => "NVML",
            GpuBackend::Wmi(_) => "WMI",
            GpuBackend::None => "none",
        }
    }

    pub fn sample(&mut self) -> Vec<GpuSnapshot> {
        match self {
            GpuBackend::Nvml(state) => sample_nvml(state),
            GpuBackend::Wmi(w) => w.sample().unwrap_or_default(),
            GpuBackend::None => Vec::new(),
        }
    }
}

fn sample_nvml(state: &mut NvmlGpu) -> Vec<GpuSnapshot> {
    let count = state.nvml.device_count().unwrap_or(0);
    // 仅对主 GPU 低频采样 PCIe（调用本身有毫秒级开销）
    if state.tick.is_multiple_of(PCIE_SAMPLE_EVERY) {
        state.pcie_cache = state.nvml.device_by_index(0).ok().map(|dev| {
            (
                dev.pcie_throughput(PcieUtilCounter::Receive).unwrap_or(0),
                dev.pcie_throughput(PcieUtilCounter::Send).unwrap_or(0),
            )
        });
    }
    state.tick = state.tick.wrapping_add(1);

    (0..count)
        .filter_map(|i| {
            let dev = state.nvml.device_by_index(i).ok()?;
            let mem = dev.memory_info().ok();
            let util = dev.utilization_rates().ok();
            let throttle = dev
                .current_throttle_reasons()
                .unwrap_or(ThrottleReasons::empty());
            let pcie = if i == 0 { state.pcie_cache } else { None };
            Some(GpuSnapshot {
                name: dev.name().unwrap_or_else(|_| format!("GPU {i}")),
                util_pct: util.as_ref().map(|u| u.gpu as f32).unwrap_or(0.0),
                vram_used: mem.as_ref().map(|m| m.used).unwrap_or(0),
                vram_total: mem.as_ref().map(|m| m.total).unwrap_or(0),
                temp_c: dev.temperature(TemperatureSensor::Gpu).ok(),
                power_w: dev.power_usage().ok().map(|mw| mw as f32 / 1000.0),
                power_limit_w: dev
                    .power_management_limit()
                    .ok()
                    .map(|mw| mw as f32 / 1000.0),
                core_mhz: dev.clock_info(Clock::Graphics).ok(),
                mem_mhz: dev.clock_info(Clock::Memory).ok(),
                fan_pct: dev.fan_speed(0).ok(),
                mem_ctrl_pct: util.as_ref().map(|u| u.memory),
                enc_pct: dev.encoder_utilization().ok().map(|e| e.utilization),
                dec_pct: dev.decoder_utilization().ok().map(|d| d.utilization),
                pcie_rx_kbs: pcie.map(|p| p.0),
                pcie_tx_kbs: pcie.map(|p| p.1),
                throttle_thermal: throttle.intersects(
                    ThrottleReasons::HW_SLOWDOWN
                        | ThrottleReasons::SW_THERMAL_SLOWDOWN
                        | ThrottleReasons::HW_THERMAL_SLOWDOWN,
                ),
                throttle_power: throttle.intersects(
                    ThrottleReasons::SW_POWER_CAP | ThrottleReasons::HW_POWER_BRAKE_SLOWDOWN,
                ),
                temp_slowdown_c: dev
                    .temperature_threshold(TemperatureThreshold::Slowdown)
                    .ok(),
                hotspot_c: None,
                fan_rpm: None,
                vram_temp_c: None,
            })
        })
        .collect()
}

// ---------- WMI 兜底路径 ----------

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine")]
#[serde(rename_all = "PascalCase")]
struct GpuEngine {
    name: String,
    utilization_percentage: u64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory")]
#[serde(rename_all = "PascalCase")]
struct GpuAdapterMemory {
    name: String,
    dedicated_usage: u64,
    /// 核显没有专用显存，占用记在共享内存字段里
    shared_usage: u64,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_VideoController")]
#[serde(rename_all = "PascalCase")]
struct VideoController {
    name: String,
}

pub struct WmiGpu {
    conn: WMIConnection,
    adapter_names: Vec<String>,
}

/// 从计数器实例名（如 "luid_0x00000000_0x0000C87B_phys_0_eng_0_engtype_3D"）提取 LUID
fn parse_luid(instance: &str) -> Option<&str> {
    let start = instance.find("luid_")?;
    let rest = &instance[start..];
    let end = rest.find("_phys").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn parse_engtype(instance: &str) -> &str {
    instance
        .rfind("engtype_")
        .map(|i| &instance[i + "engtype_".len()..])
        .unwrap_or("unknown")
}

impl WmiGpu {
    pub fn init() -> Result<Self, wmi::WMIError> {
        let com = COMLibrary::new()?;
        let conn = WMIConnection::new(com)?;
        let adapter_names = conn
            .query::<VideoController>()
            .map(|v| v.into_iter().map(|c| c.name).collect())
            .unwrap_or_default();
        Ok(WmiGpu {
            conn,
            adapter_names,
        })
    }

    pub fn sample(&self) -> Result<Vec<GpuSnapshot>, wmi::WMIError> {
        let engines: Vec<GpuEngine> = self.conn.query()?;
        let memory: Vec<GpuAdapterMemory> = self.conn.query()?;

        // 每 LUID 按引擎类型汇总占用率，取各类型中的最大值作为该适配器占用率
        let mut util_by_luid: HashMap<String, HashMap<String, u64>> = HashMap::new();
        for e in &engines {
            if let Some(luid) = parse_luid(&e.name) {
                *util_by_luid
                    .entry(luid.to_string())
                    .or_default()
                    .entry(parse_engtype(&e.name).to_string())
                    .or_default() += e.utilization_percentage;
            }
        }

        let mut dedicated_by_luid: HashMap<String, u64> = HashMap::new();
        let mut shared_by_luid: HashMap<String, u64> = HashMap::new();
        for m in &memory {
            if let Some(luid) = parse_luid(&m.name) {
                *dedicated_by_luid.entry(luid.to_string()).or_default() += m.dedicated_usage;
                *shared_by_luid.entry(luid.to_string()).or_default() += m.shared_usage;
            }
        }

        let mut luids: Vec<&String> = util_by_luid.keys().collect();
        luids.sort();
        // 存在性必须用稳定属性判定，绝不能用瞬时活动（util/vram>0）：
        // 核显无专用显存、空闲时 util=0，用活动过滤会导致适配器在快照中
        // 时隐时现，前端每秒整卡重建拖垮低端机。规则：能映射到
        // Win32_VideoController 名字的适配器恒保留；映射不到的（如
        // Microsoft 基本渲染驱动的幽灵 LUID）恒剔除——两者跨采样稳定
        Ok(luids
            .into_iter()
            .enumerate()
            .filter(|(i, _)| *i < self.adapter_names.len())
            .map(|(i, luid)| {
                let util = util_by_luid[luid]
                    .values()
                    .max()
                    .copied()
                    .unwrap_or(0)
                    .min(100) as f32;
                let dedicated = dedicated_by_luid.get(luid).copied().unwrap_or(0);
                let shared = shared_by_luid.get(luid).copied().unwrap_or(0);
                GpuSnapshot {
                    // LUID 与 VideoController 顺序无从对应，按序号取名字（尽力而为）
                    name: self
                        .adapter_names
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("GPU {i}")),
                    util_pct: util,
                    // 独显用专用显存；核显专用为 0，回退共享内存占用
                    vram_used: if dedicated > 0 { dedicated } else { shared },
                    vram_total: 0,
                    temp_c: None,
                    power_w: None,
                    power_limit_w: None,
                    core_mhz: None,
                    mem_mhz: None,
                    fan_pct: None,
                    mem_ctrl_pct: None,
                    enc_pct: None,
                    dec_pct: None,
                    pcie_rx_kbs: None,
                    pcie_tx_kbs: None,
                    throttle_thermal: false,
                    throttle_power: false,
                    temp_slowdown_c: None,
                    hotspot_c: None,
                    fan_rpm: None,
                    vram_temp_c: None,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_counter_instance_names() {
        let n = "luid_0x00000000_0x0000C87B_phys_0_eng_0_engtype_3D";
        assert_eq!(parse_luid(n), Some("luid_0x00000000_0x0000C87B"));
        assert_eq!(parse_engtype(n), "3D");
    }

    #[test]
    #[ignore = "hw: 需要 GPU 硬件（NVML 或 WMI GPU 计数器）"]
    fn backend_initializes_and_samples() {
        let mut backend = GpuBackend::init();
        let gpus = backend.sample();
        println!("GPU backend: {}, {} device(s)", backend.backend_name(), gpus.len());
        for g in &gpus {
            println!(
                "  {} util={}% vram={}/{} temp={:?} power={:?} memctrl={:?} enc={:?} dec={:?} \
                 pcie_rx={:?} slowdown_at={:?} throttle(t/p)={}/{}",
                g.name,
                g.util_pct,
                g.vram_used,
                g.vram_total,
                g.temp_c,
                g.power_w,
                g.mem_ctrl_pct,
                g.enc_pct,
                g.dec_pct,
                g.pcie_rx_kbs,
                g.temp_slowdown_c,
                g.throttle_thermal,
                g.throttle_power,
            );
            assert!((0.0..=100.0).contains(&g.util_pct));
            assert!(!g.name.is_empty());
            if let Some(m) = g.mem_ctrl_pct {
                assert!(m <= 100);
            }
        }
    }

    /// WMI 兜底路径独立验证（即使 NVML 可用也要保证 WMI 查询可跑通）
    #[test]
    #[ignore = "hw: 需要 GPU 硬件与 WMI GPU 计数器"]
    fn wmi_fallback_queries_work() {
        let w = WmiGpu::init().expect("WMI init failed");
        let gpus = w.sample().expect("WMI sample failed");
        println!("WMI path: {} adapter(s)", gpus.len());
        for g in &gpus {
            println!("  {} util={}% vram_used={}", g.name, g.util_pct, g.vram_used);
            assert!((0.0..=100.0).contains(&g.util_pct));
        }
    }
}
