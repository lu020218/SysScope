use crate::cpu_perf::{self, CpuPerf, CpuPerfSampler};
use crate::disk::{DiskSampler, StorageSnapshot};
use crate::fps::{FpsCollector, FpsSnapshot};
use crate::gpu::{GpuBackend, GpuSnapshot};
use crate::gpu_proc::GpuProcSampler;
use crate::mem_ext::{MemExt, MemExtSampler};
use crate::net_ext::{self, NetExt};
use crate::netproc::{NetProcCollector, ProcNetStat};
use crate::ping::{PingProber, PingStats};
use crate::recorder::{Recorder, RecorderCtl};
use crate::sensors::{SensorBridge, StorageTemp};
use crate::wmi_hub::WmiHub;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
    RefreshKind, System,
};
use tauri::{AppHandle, Emitter};

/// 采样间隔（毫秒），可由前端通过 set_sample_interval 调整
static SAMPLE_INTERVAL_MS: AtomicU64 = AtomicU64::new(1000);

const MIN_INTERVAL_MS: u64 = 500;
const MAX_INTERVAL_MS: u64 = 5000;
const TOP_N: usize = 5;

#[derive(Serialize, Clone)]
pub struct CpuSnapshot {
    /// 总占用率 0-100
    pub total: f32,
    /// 每逻辑核心占用率 0-100
    pub per_core: Vec<f32>,
    /// 当前频率（MHz，取各核心最大值）
    pub freq_mhz: u64,
    /// CPU 温度（摄氏度），传感器不可用时为 None
    pub temp_c: Option<f32>,
    /// CPU 包功耗（W），传感器不可用时为 None
    pub power_w: Option<f32>,
    /// 会话内峰值功耗（W）
    pub power_peak_w: Option<f32>,
    /// CPU 核心电压（V）
    pub voltage_v: Option<f32>,
    /// 每物理核心当前频率（MHz，LHM）
    pub core_clocks: Vec<f32>,
    /// 基准频率（MHz）
    pub base_mhz: u32,
    /// 有效频率（MHz，基准 × %ProcessorPerformance）
    pub effective_mhz: Option<f64>,
    /// 是否处于睿频（有效性能比 > 100%）
    pub boost: bool,
    /// C-State 驻留占比与性能比（WMI 不可用时为 None）
    pub perf: Option<CpuPerf>,
}

#[derive(Serialize, Clone)]
pub struct MemSnapshot {
    /// 单位均为字节
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// MemCompression 进程工作集（压缩存储区近似值）
    pub compression: Option<u64>,
    #[serde(flatten)]
    pub ext: MemExt,
}

#[derive(Serialize, Clone)]
pub struct NetIface {
    pub name: String,
    /// 字节每秒
    pub down_bps: f64,
    pub up_bps: f64,
    /// 系统启动以来累计字节
    pub total_rx: u64,
    pub total_tx: u64,
}

#[derive(Serialize, Clone)]
pub struct NetSnapshot {
    /// 全部活动接口聚合速率（字节每秒）
    pub down_bps: f64,
    pub up_bps: f64,
    pub total_rx: u64,
    pub total_tx: u64,
    /// 按当前速率排序的活动接口（最多 4 个）
    pub ifaces: Vec<NetIface>,
    pub ping: PingStats,
    #[serde(flatten)]
    pub ext: NetExt,
}

#[derive(Serialize, Clone)]
pub struct ProcStat {
    pub pid: u32,
    pub name: String,
    /// 全局归一化占用率（0-100，已除以核心数）
    pub cpu_pct: f32,
    /// 物理内存（字节）
    pub mem: u64,
    /// 磁盘读写速率（字节/秒）
    pub disk_bps: f64,
}

#[derive(Serialize, Clone)]
pub struct GpuProcStat {
    pub pid: u32,
    pub name: String,
    pub gpu_pct: f32,
    pub vram: u64,
}

#[derive(Serialize, Clone)]
pub struct Snapshot {
    /// Unix 时间戳（毫秒）
    pub ts: u64,
    pub cpu: CpuSnapshot,
    pub mem: MemSnapshot,
    pub gpus: Vec<GpuSnapshot>,
    pub fps: FpsSnapshot,
    pub net: NetSnapshot,
    pub storage: StorageSnapshot,
    /// 硬盘温度（来源 LHM，与 storage.disks 不做强关联）
    pub storage_temps: Vec<StorageTemp>,
    pub top_cpu: Vec<ProcStat>,
    pub top_mem: Vec<ProcStat>,
    pub top_net: Vec<ProcNetStat>,
    pub top_disk: Vec<ProcStat>,
    pub top_gpu: Vec<GpuProcStat>,
}

#[derive(Serialize, Clone)]
pub struct StaticInfo {
    pub cpu_name: String,
    pub logical_cores: usize,
    pub physical_cores: Option<usize>,
    pub total_mem: u64,
    pub os: String,
    pub hostname: String,
    /// 每逻辑核心效率等级（P 核等级高，非混合架构全相同）
    pub core_classes: Vec<u8>,
}

fn refresh_kind() -> RefreshKind {
    RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::nothing().with_cpu_usage().with_frequency())
        .with_memory(MemoryRefreshKind::nothing().with_ram().with_swap())
}

/// 采样线程持有的全部采集器
pub struct SamplerCtx {
    pub sys: System,
    pub gpu: GpuBackend,
    pub fps: FpsCollector,
    pub networks: Networks,
    /// WMI 查询入口，各 WMI 依赖的采集器共用
    pub hub: WmiHub,
    pub disk: DiskSampler,
    pub mem_ext: MemExtSampler,
    pub cpu_perf: CpuPerfSampler,
    pub gpu_proc: GpuProcSampler,
    pub sensors: Option<SensorBridge>,
    pub ping: PingProber,
    pub netproc: NetProcCollector,
    /// 会话内 CPU 峰值功耗
    pub power_peak: f32,
}

impl SamplerCtx {
    /// 必须在采样线程内构造（GPU/磁盘依赖线程 COM 环境）
    pub fn init() -> Self {
        let gpu = GpuBackend::init();
        println!("[sysscope] GPU backend: {}", gpu.backend_name());
        let fps = FpsCollector::init();
        let sensors = SensorBridge::init();
        if sensors.is_none() {
            println!("[sysscope] sensor bridge unavailable, CPU temp/power disabled");
        }
        let mut sys = System::new_with_specifics(refresh_kind());
        // 首次刷新仅用于建立 CPU 使用率基线，数据不发送
        sys.refresh_specifics(refresh_kind());
        let hub = WmiHub::new();
        let mem_ext = MemExtSampler::new(&hub);
        let cpu_perf = CpuPerfSampler::new(&hub);
        SamplerCtx {
            sys,
            gpu,
            fps,
            networks: Networks::new_with_refreshed_list(),
            hub,
            disk: DiskSampler::new(),
            mem_ext,
            cpu_perf,
            gpu_proc: GpuProcSampler::new(),
            sensors,
            ping: PingProber::spawn(),
            netproc: NetProcCollector::init(),
            power_peak: 0.0,
        }
    }
}

/// 采样网络：sysinfo 的 received/transmitted 是自上次 refresh 以来的增量，
/// 除以实际经过秒数得到速率
fn sample_net(networks: &mut Networks, elapsed_secs: f64) -> NetSnapshot {
    networks.refresh(true);
    let elapsed = if elapsed_secs > 0.0 { elapsed_secs } else { 1.0 };
    let mut ifaces: Vec<NetIface> = networks
        .iter()
        // Npcap 伪接口镜像物理网卡流量，纳入会导致双重计数
        .filter(|(name, _)| !name.contains("Loopback") && !name.contains("Npcap"))
        .map(|(name, data)| NetIface {
            name: name.clone(),
            down_bps: data.received() as f64 / elapsed,
            up_bps: data.transmitted() as f64 / elapsed,
            total_rx: data.total_received(),
            total_tx: data.total_transmitted(),
        })
        .filter(|i| i.total_rx + i.total_tx > 0)
        .collect();
    ifaces.sort_by(|a, b| {
        (b.down_bps + b.up_bps)
            .partial_cmp(&(a.down_bps + a.up_bps))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    NetSnapshot {
        down_bps: ifaces.iter().map(|i| i.down_bps).sum(),
        up_bps: ifaces.iter().map(|i| i.up_bps).sum(),
        total_rx: ifaces.iter().map(|i| i.total_rx).sum(),
        total_tx: ifaces.iter().map(|i| i.total_tx).sum(),
        ifaces: ifaces.into_iter().take(4).collect(),
        ping: PingStats::default(),
        ext: NetExt::default(),
    }
}

struct TopProcs {
    top_cpu: Vec<ProcStat>,
    top_mem: Vec<ProcStat>,
    top_disk: Vec<ProcStat>,
    top_gpu: Vec<GpuProcStat>,
    compression: Option<u64>,
}

/// CPU / 内存 / 磁盘 / GPU Top-N 进程 + MemCompression 工作集
fn sample_top_procs(
    sys: &mut System,
    elapsed_secs: f64,
    gpu_map: &std::collections::HashMap<u32, (f32, u64)>,
) -> TopProcs {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage(),
    );
    let ncores = sys.cpus().len().max(1) as f32;
    let elapsed = if elapsed_secs > 0.0 { elapsed_secs } else { 1.0 };
    let mut compression: Option<u64> = None;
    let mut procs: Vec<ProcStat> = sys
        .processes()
        .iter()
        .filter(|(pid, p)| pid.as_u32() != 0 && !p.name().is_empty())
        .map(|(pid, p)| {
            let name = p.name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case("MemCompression")
                || name.eq_ignore_ascii_case("Memory Compression")
            {
                compression = Some(p.memory());
            }
            let du = p.disk_usage();
            ProcStat {
                pid: pid.as_u32(),
                name,
                cpu_pct: p.cpu_usage() / ncores,
                mem: p.memory(),
                disk_bps: (du.read_bytes + du.written_bytes) as f64 / elapsed,
            }
        })
        .collect();

    let top_gpu: Vec<GpuProcStat> = {
        let mut v: Vec<GpuProcStat> = gpu_map
            .iter()
            .filter(|(_, (util, vram))| *util >= 0.5 || *vram > 0)
            .map(|(&pid, &(gpu_pct, vram))| GpuProcStat {
                pid,
                name: sys
                    .process(sysinfo::Pid::from_u32(pid))
                    .map(|p| p.name().to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("PID {pid}")),
                gpu_pct,
                vram,
            })
            .collect();
        v.sort_by(|a, b| {
            (b.gpu_pct, b.vram)
                .partial_cmp(&(a.gpu_pct, a.vram))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v.truncate(TOP_N);
        v
    };

    procs.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
    let top_cpu: Vec<ProcStat> = procs.iter().take(TOP_N).cloned().collect();
    procs.sort_by(|a, b| {
        b.disk_bps
            .partial_cmp(&a.disk_bps)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_disk: Vec<ProcStat> = procs.iter().take(TOP_N).cloned().collect();
    procs.sort_by(|a, b| b.mem.cmp(&a.mem));
    let top_mem: Vec<ProcStat> = procs.into_iter().take(TOP_N).collect();
    TopProcs {
        top_cpu,
        top_mem,
        top_disk,
        top_gpu,
        compression,
    }
}

pub fn take_snapshot(ctx: &mut SamplerCtx, elapsed_secs: f64) -> Snapshot {
    ctx.sys.refresh_specifics(refresh_kind());
    let fps_snapshot = ctx.fps.sample(&mut ctx.sys);
    let sensor_data = ctx
        .sensors
        .as_ref()
        .map(|s| s.read())
        .unwrap_or_default();
    let gpu_procs = ctx.gpu_proc.sample(&ctx.hub).clone();
    let tops = sample_top_procs(&mut ctx.sys, elapsed_secs, &gpu_procs);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(pw) = sensor_data.cpu_power {
        ctx.power_peak = ctx.power_peak.max(pw);
    }
    let perf = ctx.cpu_perf.sample(&ctx.hub);
    let base_mhz = ctx.cpu_perf.base_mhz();
    let effective_mhz = perf
        .as_ref()
        .filter(|_| base_mhz > 0)
        .map(|p| base_mhz as f64 * p.perf_pct / 100.0);
    Snapshot {
        ts,
        cpu: CpuSnapshot {
            total: ctx.sys.global_cpu_usage(),
            per_core: ctx.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            freq_mhz: ctx.sys.cpus().iter().map(|c| c.frequency()).max().unwrap_or(0),
            temp_c: sensor_data.cpu_temp,
            power_w: sensor_data.cpu_power,
            power_peak_w: (ctx.power_peak > 0.0).then_some(ctx.power_peak),
            voltage_v: sensor_data.cpu_voltage,
            core_clocks: sensor_data.core_clocks,
            base_mhz,
            effective_mhz,
            boost: perf.as_ref().map(|p| p.perf_pct > 100.0).unwrap_or(false),
            perf,
        },
        mem: MemSnapshot {
            total: ctx.sys.total_memory(),
            used: ctx.sys.used_memory(),
            available: ctx.sys.available_memory(),
            swap_total: ctx.sys.total_swap(),
            swap_used: ctx.sys.used_swap(),
            compression: tops.compression,
            ext: ctx.mem_ext.sample(&ctx.hub),
        },
        gpus: {
            let mut gpus = ctx.gpu.sample();
            // LHM/NVAPI 侧传感器仅覆盖主 GPU
            if let Some(first) = gpus.first_mut() {
                first.hotspot_c = sensor_data.gpu_hotspot;
                first.fan_rpm = sensor_data.gpu_fan_rpm;
                first.vram_temp_c = sensor_data.gpu_vram_temp;
            }
            gpus
        },
        fps: fps_snapshot,
        net: {
            let mut net = sample_net(&mut ctx.networks, elapsed_secs);
            net.ping = ctx.ping.stats();
            net.ext = net_ext::sample(&ctx.hub);
            net
        },
        storage: ctx.disk.sample(&ctx.hub),
        storage_temps: sensor_data.storage,
        top_net: ctx.netproc.sample(&ctx.sys, elapsed_secs),
        top_cpu: tops.top_cpu,
        top_mem: tops.top_mem,
        top_disk: tops.top_disk,
        top_gpu: tops.top_gpu,
    }
}

#[tauri::command]
pub fn get_static_info() -> StaticInfo {
    let sys = System::new_with_specifics(refresh_kind());
    StaticInfo {
        cpu_name: sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_else(|| "Unknown CPU".into()),
        logical_cores: sys.cpus().len(),
        physical_cores: sys.physical_core_count(),
        total_mem: sys.total_memory(),
        os: format!(
            "{} {}",
            System::name().unwrap_or_default(),
            System::os_version().unwrap_or_default()
        ),
        hostname: System::host_name().unwrap_or_default(),
        core_classes: cpu_perf::core_efficiency_classes(),
    }
}

#[tauri::command]
pub fn set_sample_interval(ms: u64) {
    SAMPLE_INTERVAL_MS.store(ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS), Ordering::Relaxed);
}

/// 采样主循环（不返回，除非内部 panic 逃逸）
fn run_sampler(app: &AppHandle, rec_ctl: &Arc<RecorderCtl>, db_path: &PathBuf) {
    let mut recorder = Recorder::new(rec_ctl.clone(), db_path.clone());
    let mut ctx = SamplerCtx::init();
    let mut last_tick = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(
            SAMPLE_INTERVAL_MS.load(Ordering::Relaxed),
        ));
        let elapsed = last_tick.elapsed().as_secs_f64();
        last_tick = Instant::now();
        let snapshot = take_snapshot(&mut ctx, elapsed);
        recorder.tick(&snapshot);
        let _ = app.emit("metrics", &snapshot);
    }
}

/// 启动后台采样线程：按当前间隔采样并向前端广播 "metrics" 事件；
/// 录制开启时同步写入 SQLite。
/// 守护外壳：任何 panic 被捕获后通知前端并在 3 秒后重建采集器继续，
/// 避免单次异常让监控静默死亡
pub fn spawn(app: AppHandle, rec_ctl: Arc<RecorderCtl>, db_path: PathBuf) {
    std::thread::spawn(move || loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_sampler(&app, &rec_ctl, &db_path);
        }));
        if let Err(e) = result {
            eprintln!("[sysscope] sampler panicked, restarting in 3s: {e:?}");
            let _ = app.emit("sampler-crashed", ());
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_produces_plausible_values() {
        let mut ctx = SamplerCtx::init();
        std::thread::sleep(Duration::from_millis(600));
        let s = take_snapshot(&mut ctx, 0.6);

        assert!(s.ts > 0);
        assert!(!s.cpu.per_core.is_empty());
        assert!((0.0..=100.0).contains(&s.cpu.total));
        for core in &s.cpu.per_core {
            assert!((0.0..=100.0).contains(core));
        }
        assert!(s.mem.total > 0);
        assert!(s.mem.used > 0 && s.mem.used <= s.mem.total);
        assert!(s.mem.available <= s.mem.total);

        assert!(s.net.down_bps >= 0.0 && s.net.up_bps >= 0.0);
        for i in &s.net.ifaces {
            assert!(!i.name.is_empty());
            assert!(!i.name.contains("Loopback"));
        }

        // 磁盘与 Top-N（第二次采样后进程 CPU 值才有意义，这里只做结构断言）
        assert!(!s.storage.volumes.is_empty());
        assert!(!s.top_mem.is_empty());
        assert!(s.top_cpu.len() <= TOP_N && s.top_mem.len() <= TOP_N);
        for p in s.top_cpu.iter().chain(s.top_mem.iter()) {
            assert!(p.pid > 0 && !p.name.is_empty());
            assert!((0.0..=100.0).contains(&p.cpu_pct));
        }
        println!(
            "cpu={}% power={:?}W top_mem[0]={} {}MB disks={} temps={}",
            s.cpu.total,
            s.cpu.power_w,
            s.top_mem[0].name,
            s.top_mem[0].mem >> 20,
            s.storage.disks.len(),
            s.storage_temps.len(),
        );
    }

    #[test]
    fn static_info_is_populated() {
        let info = get_static_info();
        assert!(!info.cpu_name.is_empty());
        assert!(info.logical_cores > 0);
        assert!(info.total_mem > 0);
    }
}
