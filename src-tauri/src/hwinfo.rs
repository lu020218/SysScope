//! 硬件信息：静态规格数据（CPU / 内存 / 主板 / BIOS / 整机 / 系统）。
//!
//! 与其余采集器的关键区别是**采集时机**：这些数据永不变化，因此只在首次
//! 请求时查一次并永久缓存，绝不进入采样循环，也不放进采样线程的初始化
//! （那会让 221ms 的启动预算直接翻倍）。
//!
//! 数据源以 WMI 类实例查询为主 —— 注意这与当年拖垮性能的
//! `Win32_PerfFormattedData_*` 格式化计数器不是一回事：后者每次查询都要
//! 阻塞等待一个内部采样窗口，前者只是读一次 CIM 仓库。即便如此，十来个类
//! 加起来仍有数百毫秒，所以必须缓存。

use crate::wmi_hub::WmiHub;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ---------- 对外数据结构 ----------

/// 键值对。value 为 None 表示该项在本机取不到，前端显示 N/A。
/// key 是 i18n key，由前端/报告翻译；value 是设备原文，不翻译。
#[derive(Serialize, Clone, Debug)]
pub struct HwItem {
    pub key: String,
    pub value: Option<String>,
    /// 该值是否为机器指纹（序列号、MAC 等），导出报告时默认脱敏
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sensitive: bool,
}

/// OEM/主板厂在没有真实值时填进 DMI 的占位串。DIY 装机极常见，
/// 原样展示只会让人以为是真的型号。
const PLACEHOLDERS: &[&str] = &[
    "system product name",
    "system manufacturer",
    "system version",
    "system serial number",
    "to be filled by o.e.m.",
    "default string",
    "not specified",
    "not available",
    "none",
    "o.e.m.",
    "unknown",
];

fn meaningful(v: String) -> Option<String> {
    let t = v.trim();
    let lower = t.to_ascii_lowercase();
    // 全 0 的序列号（内存条极常见）同样无意义
    let all_zero = !t.is_empty() && t.chars().all(|c| c == '0');
    (!t.is_empty() && !all_zero && !PLACEHOLDERS.contains(&lower.as_str()))
        .then(|| t.to_string())
}

impl HwItem {
    fn new(key: &str, value: Option<String>) -> Self {
        HwItem {
            key: key.into(),
            value: value.and_then(meaningful),
            sensitive: false,
        }
    }

    fn secret(key: &str, value: Option<String>) -> Self {
        HwItem {
            sensitive: true,
            ..HwItem::new(key, value)
        }
    }
}

/// 一组硬件（如一条内存、一块磁盘）：标题 + 若干条目
#[derive(Serialize, Clone, Debug)]
pub struct HwGroup {
    /// 组标题。空串表示该分类只有一组，前端不渲染小标题
    pub title: String,
    pub items: Vec<HwItem>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct HwInfo {
    pub cpu: Vec<HwGroup>,
    pub memory: Vec<HwGroup>,
    pub gpu: Vec<HwGroup>,
    pub disk: Vec<HwGroup>,
    pub network: Vec<HwGroup>,
    pub board: Vec<HwGroup>,
    pub system: Vec<HwGroup>,
}

// ---------- WMI 行定义 ----------

#[derive(Deserialize)]
#[serde(rename = "Win32_Processor")]
#[serde(rename_all = "PascalCase")]
struct Processor {
    name: Option<String>,
    socket_designation: Option<String>,
    number_of_cores: Option<u32>,
    number_of_logical_processors: Option<u32>,
    max_clock_speed: Option<u32>,
    l2_cache_size: Option<u32>,
    l3_cache_size: Option<u32>,
    description: Option<String>,
    virtualization_firmware_enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PhysicalMemory")]
#[serde(rename_all = "PascalCase")]
struct PhysicalMemory {
    capacity: Option<u64>,
    manufacturer: Option<String>,
    part_number: Option<String>,
    speed: Option<u32>,
    configured_clock_speed: Option<u32>,
    device_locator: Option<String>,
    bank_label: Option<String>,
    #[serde(rename = "SMBIOSMemoryType")]
    smbios_memory_type: Option<u32>,
    serial_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_PhysicalMemoryArray")]
#[serde(rename_all = "PascalCase")]
struct MemoryArray {
    memory_devices: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_VideoController")]
#[serde(rename_all = "PascalCase")]
struct VideoController {
    name: Option<String>,
    driver_version: Option<String>,
    driver_date: Option<wmi::WMIDateTime>,
    video_processor: Option<String>,
    adapter_compatibility: Option<String>,
}

/// 存储命名空间（root\Microsoft\Windows\Storage）。相比 Win32_DiskDrive，
/// 它能直接给出 SSD/HDD 判定与总线类型，不必从型号字符串猜。
#[derive(Deserialize)]
#[serde(rename = "MSFT_PhysicalDisk")]
#[serde(rename_all = "PascalCase")]
struct PhysicalDisk {
    friendly_name: Option<String>,
    serial_number: Option<String>,
    firmware_version: Option<String>,
    size: Option<u64>,
    media_type: Option<u16>,
    bus_type: Option<u16>,
    spindle_speed: Option<u32>,
    health_status: Option<u16>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_NetworkAdapter")]
#[serde(rename_all = "PascalCase")]
struct NetworkAdapter {
    #[serde(rename = "MACAddress")]
    mac_address: Option<String>,
    #[serde(rename = "PNPDeviceID")]
    pnp_device_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_BaseBoard")]
#[serde(rename_all = "PascalCase")]
struct BaseBoard {
    manufacturer: Option<String>,
    product: Option<String>,
    version: Option<String>,
    serial_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_BIOS")]
#[serde(rename_all = "PascalCase")]
struct Bios {
    manufacturer: Option<String>,
    #[serde(rename = "SMBIOSBIOSVersion")]
    smbios_bios_version: Option<String>,
    release_date: Option<wmi::WMIDateTime>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_ComputerSystem")]
#[serde(rename_all = "PascalCase")]
struct ComputerSystem {
    manufacturer: Option<String>,
    model: Option<String>,
    hypervisor_present: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_OperatingSystem")]
#[serde(rename_all = "PascalCase")]
struct OperatingSystem {
    caption: Option<String>,
    version: Option<String>,
    build_number: Option<String>,
    os_architecture: Option<String>,
    install_date: Option<wmi::WMIDateTime>,
}

// ---------- 辅助 ----------

fn bytes_gb(b: u64) -> String {
    format!("{:.0} GB", b as f64 / 1024.0 / 1024.0 / 1024.0)
}

/// SMBIOS 内存类型码 → 名称（SMBIOS 3.x 规范表 7.18.2）
fn ddr_name(code: u32) -> Option<&'static str> {
    match code {
        20 => Some("DDR"),
        21 => Some("DDR2"),
        24 => Some("DDR3"),
        26 => Some("DDR4"),
        34 => Some("DDR5"),
        35 => Some("LPDDR4"),
        36 => Some("LPDDR5"),
        _ => None,
    }
}

/// 编译期已知的 x86 指令集扩展中，挑选对性能判断有意义的几项做运行时探测。
/// 走 std 宏读 CPUID，不经 WMI，零成本。
fn cpu_features() -> String {
    let mut out = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        for (name, present) in [
            ("SSE4.2", is_x86_feature_detected!("sse4.2")),
            ("AES", is_x86_feature_detected!("aes")),
            ("AVX", is_x86_feature_detected!("avx")),
            ("AVX2", is_x86_feature_detected!("avx2")),
            ("FMA", is_x86_feature_detected!("fma")),
            ("AVX-512F", is_x86_feature_detected!("avx512f")),
            ("SHA", is_x86_feature_detected!("sha")),
        ] {
            if present {
                out.push(name);
            }
        }
    }
    out.join(" · ")
}

/// 微码版本：注册表 HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0
/// 的 "Update Revision"（REG_BINARY，小端）。
///
/// 长度因平台而异：实测 Intel 13/14 代返回 4 字节，另一些平台返回 8 字节且
/// 修订号在高 4 字节（低位保留为 0）。所以按实际返回长度判断，不能写死。
fn microcode() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        RRF_RT_REG_BINARY,
    };
    unsafe {
        let mut key = HKEY::default();
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            0,
            KEY_READ,
            &mut key,
        )
        .ok()
        .ok()?;
        let mut buf = [0u8; 8];
        let mut size = buf.len() as u32;
        let st = RegGetValueW(
            key,
            None,
            w!("Update Revision"),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        st.ok().ok()?;
        let rev = match size {
            4 => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            8 => {
                let hi = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
                let lo = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                if hi != 0 { hi } else { lo }
            }
            _ => return None,
        };
        (rev != 0).then(|| format!("0x{rev:X}"))
    }
}

/// PawnIO 驱动状态。LHM 0.9.6 起用 PawnIO 取代 WinRing0 读取 MSR / SuperIO，
/// 而 PawnIO 由用户单独安装 —— 缺失时温度、功耗、风扇转速会静默变成 N/A。
/// 静默降级正是"换了散热器温度就没了"那类误判的来源，所以这里显式报出状态，
/// 让"没装驱动"和"这块硬件没有该传感器"在界面上可区分。
///
/// 读注册表而不是试着打开 \?\GLOBALROOT\Device\PawnIO：设备打开需要管理员，
/// 未提权运行时会把"已安装"误报成"未安装"。
fn pawnio_status() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };
    unsafe {
        // 驱动服务键是"装没装"的判据；Uninstall 键只用于取版本号显示，
        // 取不到版本不代表没装（安装方式不同可能没写 Uninstall 项）
        let mut svc = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Services\PawnIO"),
            0,
            KEY_READ,
            &mut svc,
        )
        .is_err()
        {
            return Some("hw.sensor.driverMissing".into());
        }
        let _ = RegCloseKey(svc);

        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO"),
            0,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return Some("hw.sensor.driverOk".into());
        }
        let mut buf = [0u16; 64];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let st = RegGetValueW(
            key,
            None,
            w!("DisplayVersion"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if st.is_err() {
            return Some("hw.sensor.driverOk".into());
        }
        // size 是字节数且含结尾 NUL
        let chars = (size as usize / 2).saturating_sub(1).min(buf.len());
        let ver = String::from_utf16_lossy(&buf[..chars]);
        let ver = ver.trim();
        if ver.is_empty() {
            Some("hw.sensor.driverOk".into())
        } else {
            Some(format!("PawnIO {ver}"))
        }
    }
}

/// 开机时长。用 GetTickCount64 而非 Win32_OperatingSystem.LastBootUpTime：
/// 免一次 WMI 往返，且不受系统时钟调整影响。
fn uptime() -> String {
    let ms = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    let secs = ms / 1000;
    format!("{}d {}h {}m", secs / 86400, secs % 86400 / 3600, secs % 3600 / 60)
}

// ---------- 采集 ----------

/// 虚拟化状态。
///
/// 不能直接用 Win32_Processor.VirtualizationFirmwareEnabled：当 Windows 自身
/// 运行在 Hyper-V 之上（启用了 Hyper-V / WSL2 / 内存完整性 VBS 等）时，CPU 的
/// 虚拟化位会被 hypervisor 屏蔽，该属性变成 False —— 在一台虚拟化明确开着的
/// 机器上显示"已关闭"，比不显示更糟。HypervisorPresent 为真即可断定已启用。
fn virtualization_state(hub: &WmiHub, fw_enabled: Option<bool>) -> Option<String> {
    let hyperv = hub
        .query::<ComputerSystem>()
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.hypervisor_present);
    let key = match (hyperv, fw_enabled) {
        (Some(true), _) => "hw.virt.hypervisor",
        (_, Some(true)) => "hw.virt.enabled",
        (_, Some(false)) => "hw.virt.disabled",
        _ => return None,
    };
    Some(key.to_string())
}

fn collect_cpu(hub: &WmiHub) -> Vec<HwGroup> {
    let procs = hub.query::<Processor>().unwrap_or_default();
    let mut groups = Vec::new();
    // 多路服务器会有多颗物理 CPU，各成一组
    let multi = procs.len() > 1;
    for (i, p) in procs.iter().enumerate() {
        let kb = |v: Option<u32>| v.map(|k| format!("{} MB", k as f64 / 1024.0));
        groups.push(HwGroup {
            title: if multi { format!("CPU {i}") } else { String::new() },
            items: vec![
                HwItem::new("hw.cpu.model", p.name.clone()),
                HwItem::new("hw.cpu.socket", p.socket_designation.clone()),
                HwItem::new(
                    "hw.cpu.cores",
                    match (p.number_of_cores, p.number_of_logical_processors) {
                        (Some(c), Some(t)) => Some(format!("{c} C / {t} T")),
                        _ => None,
                    },
                ),
                HwItem::new(
                    "hw.cpu.baseClock",
                    p.max_clock_speed.map(|m| format!("{m} MHz")),
                ),
                HwItem::new("hw.cpu.l2", kb(p.l2_cache_size)),
                HwItem::new("hw.cpu.l3", kb(p.l3_cache_size)),
                HwItem::new("hw.cpu.family", p.description.clone()),
                HwItem::new("hw.cpu.microcode", microcode()),
                HwItem::new(
                    "hw.cpu.virtualization",
                    virtualization_state(hub, p.virtualization_firmware_enabled),
                ),
                HwItem::new("hw.cpu.features", Some(cpu_features())),
            ],
        });
    }
    groups
}

fn collect_memory(hub: &WmiHub) -> Vec<HwGroup> {
    let dimms = hub.query::<PhysicalMemory>().unwrap_or_default();
    let slots = hub
        .query::<MemoryArray>()
        .and_then(|a| a.into_iter().next())
        .and_then(|a| a.memory_devices);
    let total: u64 = dimms.iter().filter_map(|d| d.capacity).sum();

    let mut groups = vec![HwGroup {
        title: String::new(),
        items: vec![
            HwItem::new(
                "hw.mem.total",
                (total > 0).then(|| bytes_gb(total)),
            ),
            HwItem::new(
                "hw.mem.slots",
                slots.map(|s| format!("{} / {}", dimms.len(), s)),
            ),
            HwItem::new(
                "hw.mem.type",
                dimms
                    .iter()
                    .find_map(|d| d.smbios_memory_type.and_then(ddr_name))
                    .map(String::from),
            ),
        ],
    }];

    for d in &dimms {
        // 插槽位标识：DeviceLocator 更具体（如 "DIMM 0"），BankLabel 兜底
        let title = d
            .device_locator
            .clone()
            .or_else(|| d.bank_label.clone())
            .unwrap_or_else(|| "DIMM".into());
        groups.push(HwGroup {
            title,
            items: vec![
                HwItem::new("hw.mem.capacity", d.capacity.map(bytes_gb)),
                HwItem::new("hw.mem.manufacturer", d.manufacturer.clone()),
                HwItem::new("hw.mem.partNumber", d.part_number.clone()),
                // 颗粒标称速率与 BIOS 实配速率不一定相同（未开 XMP 时差别很大）
                HwItem::new("hw.mem.speed", d.speed.map(|s| format!("{s} MT/s"))),
                HwItem::new(
                    "hw.mem.configuredSpeed",
                    d.configured_clock_speed.map(|s| format!("{s} MT/s")),
                ),
                HwItem::secret("hw.mem.serial", d.serial_number.clone()),
            ],
        });
    }
    groups
}

/// MSFT_PhysicalDisk.MediaType（存储 WMI 提供程序枚举）
fn media_name(code: u16) -> Option<&'static str> {
    match code {
        3 => Some("HDD"),
        4 => Some("SSD"),
        5 => Some("SCM"),
        _ => None,
    }
}

/// MSFT_PhysicalDisk.BusType
fn bus_name(code: u16) -> Option<&'static str> {
    match code {
        1 => Some("SCSI"),
        3 => Some("ATA"),
        7 => Some("USB"),
        8 => Some("RAID"),
        10 => Some("SAS"),
        11 => Some("SATA"),
        12 => Some("SD"),
        13 => Some("MMC"),
        17 => Some("NVMe"),
        _ => None,
    }
}

fn health_key(code: u16) -> &'static str {
    match code {
        0 => "hw.disk.healthy",
        1 => "hw.disk.warning",
        _ => "hw.disk.unhealthy",
    }
}

fn collect_gpu(hub: &WmiHub) -> Vec<HwGroup> {
    let controllers = hub.query::<VideoController>().unwrap_or_default();
    // NVML 只覆盖 N 卡；按索引与 WMI 列表对齐不可靠（核显也在 WMI 列表里），
    // 因此只用它补充第一块 NVIDIA 设备的信息，并单独成组
    let nvml = nvml_wrapper::Nvml::init().ok();

    let mut groups: Vec<HwGroup> = controllers
        .iter()
        .map(|c| HwGroup {
            title: c.name.clone().unwrap_or_default(),
            items: vec![
                HwItem::new("hw.gpu.vendor", c.adapter_compatibility.clone()),
                HwItem::new("hw.gpu.chip", c.video_processor.clone()),
                HwItem::new("hw.gpu.driver", c.driver_version.clone()),
                HwItem::new(
                    "hw.gpu.driverDate",
                    c.driver_date
                        .as_ref()
                        .map(|d| d.0.format("%Y-%m-%d").to_string()),
                ),
            ],
        })
        .collect();

    // 显存容量必须走 NVML/DXGI：Win32_VideoController.AdapterRAM 是 u32，
    // 超过 4GB 会回绕（16GB 显卡会报出错误值），不能用
    if let Some(nvml) = &nvml {
        if let Ok(dev) = nvml.device_by_index(0) {
            let link = |cur: Option<u32>, max: Option<u32>| match (cur, max) {
                (Some(c), Some(m)) => Some(format!("{c} / {m}")),
                (c, m) => c.or(m).map(|v| v.to_string()),
            };
            let name = dev.name().unwrap_or_else(|_| "NVIDIA".into());
            let mut extra = vec![
                    HwItem::new(
                        "hw.gpu.vram",
                        dev.memory_info().ok().map(|m| bytes_gb(m.total)),
                    ),
                    HwItem::new(
                        "hw.gpu.nvmlDriver",
                        nvml.sys_driver_version().ok(),
                    ),
                    HwItem::new("hw.gpu.vbios", dev.vbios_version().ok()),
                    HwItem::new(
                        "hw.gpu.pcieGen",
                        link(
                            dev.current_pcie_link_gen().ok(),
                            dev.max_pcie_link_gen().ok(),
                        ),
                    ),
                    HwItem::new(
                        "hw.gpu.pcieWidth",
                        link(
                            dev.current_pcie_link_width().ok(),
                            dev.max_pcie_link_width().ok(),
                        ),
                    ),
            ];
            // NVML 与 WMI 会报出同一块卡。合并进同名分组，避免界面上出现
            // 两个标题相同的分组；匹配不上（改名、多卡）时才单独成组。
            match groups.iter_mut().find(|g| g.title == name) {
                Some(g) => g.items.append(&mut extra),
                None => groups.push(HwGroup {
                    title: name,
                    items: extra,
                }),
            }
        }
    }
    groups
}

fn collect_disk() -> Vec<HwGroup> {
    // 存储类不在默认命名空间；连接失败时返回空列表，前端显示为空分类
    let hub = WmiHub::with_namespace(r"root\Microsoft\Windows\Storage");
    hub.query::<PhysicalDisk>()
        .unwrap_or_default()
        .iter()
        .map(|d| {
            let mut items = vec![
                HwItem::new("hw.disk.capacity", d.size.map(bytes_gb)),
                HwItem::new(
                    "hw.disk.media",
                    d.media_type.and_then(media_name).map(String::from),
                ),
                HwItem::new(
                    "hw.disk.bus",
                    d.bus_type.and_then(bus_name).map(String::from),
                ),
            ];
            // 转速只对机械盘有意义。SSD 上它不是"取不到"而是"不适用"，
            // 显示成 N/A 会让人以为读取失败，所以整项不生成。
            if let Some(rpm) = d.spindle_speed.filter(|s| *s > 0 && *s != u32::MAX) {
                items.push(HwItem::new("hw.disk.rpm", Some(format!("{rpm} RPM"))));
            }
            items.extend([
                HwItem::new("hw.disk.firmware", d.firmware_version.clone()),
                HwItem::new(
                    "hw.disk.health",
                    d.health_status.map(|h| health_key(h).to_string()),
                ),
                HwItem::secret("hw.disk.serial", d.serial_number.clone()),
            ]);
            HwGroup {
                title: d.friendly_name.clone().unwrap_or_default(),
                items,
            }
        })
        .collect()
}

/// sockaddr → 可读地址串。手工格式化而非 WSAAddressToStringW：
/// 免去 WinSock 初始化，且不必处理它附带的端口后缀。
unsafe fn sockaddr_str(sa: *const windows::Win32::Networking::WinSock::SOCKADDR) -> Option<String> {
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6};
    if sa.is_null() {
        return None;
    }
    match (*sa).sa_family {
        AF_INET => {
            let v4 = &*(sa as *const SOCKADDR_IN);
            let o = v4.sin_addr.S_un.S_un_b;
            Some(std::net::Ipv4Addr::new(o.s_b1, o.s_b2, o.s_b3, o.s_b4).to_string())
        }
        AF_INET6 => {
            let v6 = &*(sa as *const SOCKADDR_IN6);
            Some(std::net::Ipv6Addr::from(v6.sin6_addr.u.Byte).to_string())
        }
        _ => None,
    }
}

unsafe fn pwstr_str(p: windows::core::PWSTR) -> Option<String> {
    (!p.is_null()).then(|| p.to_string().unwrap_or_default())
}

/// IF_TYPE_* → 名称。只区分对用户有意义的几类。
fn if_type_key(t: u32) -> Option<&'static str> {
    match t {
        6 => Some("hw.net.ethernet"),
        71 => Some("hw.net.wifi"),
        131 => Some("hw.net.tunnel"),
        237 => Some("hw.net.wwan"),
        _ => None,
    }
}

/// 网卡信息走 IpHelper 的 GetAdaptersAddresses，而非 Win32_NetworkAdapter：
/// 一次调用即可拿到 MAC、IP、网关、DNS 与链路速率，比多次 WMI 往返快得多。
/// 物理网卡的 MAC 集合。
///
/// 判据用 PNPDeviceID 前缀而非 Win32_NetworkAdapter.PhysicalAdapter —— 后者
/// 对不少虚拟适配器同样返回 True（众所周知不可靠）。真实硬件挂在 PCI 或 USB
/// 总线上，而 VPN/隧道/Wi-Fi Direct 这类软件适配器是 ROOT\ 或 SWD\ 开头。
fn physical_macs(hub: &WmiHub) -> std::collections::HashSet<String> {
    hub.query::<NetworkAdapter>()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            a.pnp_device_id.as_deref().is_some_and(|id| {
                let id = id.to_ascii_uppercase();
                id.starts_with(r"PCI\") || id.starts_with(r"USB\")
            })
        })
        .filter_map(|a| a.mac_address.map(|m| m.to_ascii_uppercase()))
        .collect()
}

fn collect_net(hub: &WmiHub) -> Vec<HwGroup> {
    let physical = physical_macs(hub);
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    unsafe {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_INCLUDE_GATEWAYS;
        let mut size = 0u32;
        // 首次调用只为取所需缓冲长度
        GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);
        if size == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; size as usize];
        let head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        if GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(head), &mut size) != 0 {
            return Vec::new();
        }

        let mut groups = Vec::new();
        let mut cur = head;
        while !cur.is_null() {
            let a = &*cur;
            cur = a.Next;

            let mac = (a.PhysicalAddressLength > 0).then(|| {
                a.PhysicalAddress[..a.PhysicalAddressLength as usize]
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(":")
            });

            // 只保留真实硬件：Wi-Fi Direct、VPN 隧道、TAP 之类的软件适配器
            // 会把列表撑到十来项，还会报出 100Gbps 这样的假链路速率。
            // physical 为空说明 WMI 查询失败，此时不过滤，宁可多显示也不空白。
            let is_physical = physical.is_empty()
                || mac.as_deref().is_some_and(|m| physical.contains(m));
            if a.IfType == 24 || !is_physical {
                continue;
            }

            let mut ips = Vec::new();
            let mut uni = a.FirstUnicastAddress;
            while !uni.is_null() {
                if let Some(ip) = sockaddr_str((*uni).Address.lpSockaddr) {
                    ips.push(ip);
                }
                uni = (*uni).Next;
            }
            let mut gateways = Vec::new();
            let mut gw = a.FirstGatewayAddress;
            while !gw.is_null() {
                if let Some(ip) = sockaddr_str((*gw).Address.lpSockaddr) {
                    gateways.push(ip);
                }
                gw = (*gw).Next;
            }
            let mut dns = Vec::new();
            let mut d = a.FirstDnsServerAddress;
            while !d.is_null() {
                if let Some(ip) = sockaddr_str((*d).Address.lpSockaddr) {
                    dns.push(ip);
                }
                d = (*d).Next;
            }

            let join = |v: Vec<String>| (!v.is_empty()).then(|| v.join(", "));
            groups.push(HwGroup {
                title: pwstr_str(a.FriendlyName).unwrap_or_default(),
                items: vec![
                    HwItem::new("hw.net.description", pwstr_str(a.Description)),
                    HwItem::new(
                        "hw.net.type",
                        if_type_key(a.IfType).map(String::from),
                    ),
                    HwItem::new(
                        "hw.net.status",
                        Some(
                            if a.OperStatus.0 == 1 { "hw.net.up" } else { "hw.net.down" }
                                .to_string(),
                        ),
                    ),
                    HwItem::new(
                        "hw.net.linkSpeed",
                        (a.TransmitLinkSpeed > 0 && a.TransmitLinkSpeed != u64::MAX)
                            .then(|| format!("{} Mbps", a.TransmitLinkSpeed / 1_000_000)),
                    ),
                    HwItem::secret("hw.net.mac", mac),
                    HwItem::secret("hw.net.ip", join(ips)),
                    HwItem::new("hw.net.gateway", join(gateways)),
                    HwItem::new("hw.net.dns", join(dns)),
                ],
            });
        }
        groups
    }
}

fn collect_board(hub: &WmiHub) -> Vec<HwGroup> {
    let board = hub
        .query::<BaseBoard>()
        .and_then(|b| b.into_iter().next());
    let bios = hub.query::<Bios>().and_then(|b| b.into_iter().next());
    let cs = hub
        .query::<ComputerSystem>()
        .and_then(|c| c.into_iter().next());

    vec![
        HwGroup {
            title: String::new(),
            items: vec![
                HwItem::new(
                    "hw.board.manufacturer",
                    board.as_ref().and_then(|b| b.manufacturer.clone()),
                ),
                HwItem::new(
                    "hw.board.product",
                    board.as_ref().and_then(|b| b.product.clone()),
                ),
                HwItem::new(
                    "hw.board.version",
                    board.as_ref().and_then(|b| b.version.clone()),
                ),
                HwItem::secret(
                    "hw.board.serial",
                    board.as_ref().and_then(|b| b.serial_number.clone()),
                ),
            ],
        },
        HwGroup {
            title: "BIOS".into(),
            items: vec![
                HwItem::new(
                    "hw.bios.vendor",
                    bios.as_ref().and_then(|b| b.manufacturer.clone()),
                ),
                HwItem::new(
                    "hw.bios.version",
                    bios.as_ref().and_then(|b| b.smbios_bios_version.clone()),
                ),
                HwItem::new(
                    "hw.bios.date",
                    bios.as_ref()
                        .and_then(|b| b.release_date.as_ref())
                        .map(|d| d.0.format("%Y-%m-%d").to_string()),
                ),
                HwItem::new(
                    "hw.system.manufacturer",
                    cs.as_ref().and_then(|c| c.manufacturer.clone()),
                ),
                HwItem::new("hw.system.model", cs.as_ref().and_then(|c| c.model.clone())),
            ],
        },
    ]
}

fn collect_system(hub: &WmiHub) -> Vec<HwGroup> {
    let os = hub
        .query::<OperatingSystem>()
        .and_then(|o| o.into_iter().next());
    vec![HwGroup {
        title: String::new(),
        items: vec![
            HwItem::new("hw.os.name", os.as_ref().and_then(|o| o.caption.clone())),
            HwItem::new(
                "hw.os.build",
                match (
                    os.as_ref().and_then(|o| o.version.clone()),
                    os.as_ref().and_then(|o| o.build_number.clone()),
                ) {
                    (Some(v), Some(b)) => Some(format!("{v} (build {b})")),
                    (v, b) => v.or(b),
                },
            ),
            HwItem::new(
                "hw.os.arch",
                os.as_ref().and_then(|o| o.os_architecture.clone()),
            ),
            HwItem::new(
                "hw.os.installed",
                os.as_ref()
                    .and_then(|o| o.install_date.as_ref())
                    .map(|d| d.0.format("%Y-%m-%d").to_string()),
            ),
            HwItem::new("hw.os.uptime", Some(uptime())),
            HwItem::new("hw.os.hostname", sysinfo::System::host_name()),
            HwItem::new("hw.os.sensorDriver", pawnio_status()),
        ],
    }]
}

fn collect() -> HwInfo {
    // WmiHub 依赖线程 COM 环境，必须在本线程创建，不能复用采样线程那个
    let hub = WmiHub::new();
    HwInfo {
        cpu: collect_cpu(&hub),
        memory: collect_memory(&hub),
        gpu: collect_gpu(&hub),
        disk: collect_disk(),
        network: collect_net(&hub),
        board: collect_board(&hub),
        system: collect_system(&hub),
    }
}

/// 传感器驱动是否缺失。前端据此显示安装引导 —— 缺驱动时温度、功耗、
/// 每核心频率与主板风扇全部变 N/A，不给出原因的话与"这台机器没有这些
/// 传感器"无从区分。
#[tauri::command]
pub fn sensor_driver_missing() -> bool {
    matches!(pawnio_status().as_deref(), Some("hw.sensor.driverMissing"))
}

/// 打开 PawnIO 官网。固定 URL 而非接受前端传入的任意地址 —— 这个命令
/// 只有一个用途，没有理由把"打开任意 URL"的能力暴露给渲染层。
#[tauri::command]
pub fn open_sensor_driver_site() {
    use windows::core::w;
    use windows::Win32::UI::Shell::ShellExecuteW;
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("https://pawnio.eu/"),
            None,
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
    }
}

static CACHE: OnceLock<HwInfo> = OnceLock::new();

/// 硬件信息（首次调用查询并永久缓存；后续为纯内存读取）。
/// 放在阻塞线程池上执行 —— WMI 往返数百毫秒，不能占用异步执行器。
#[tauri::command]
pub async fn hardware_info() -> HwInfo {
    tauri::async_runtime::spawn_blocking(|| CACHE.get_or_init(collect).clone())
        .await
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddr_names_cover_current_generations() {
        assert_eq!(ddr_name(26), Some("DDR4"));
        assert_eq!(ddr_name(34), Some("DDR5"));
        // 未知码返回 None 而非猜测，前端显示 N/A
        assert_eq!(ddr_name(999), None);
    }

    #[test]
    fn empty_values_collapse_to_none() {
        // OEM 常把取不到的字段填成空串或空格，不能当成有效值展示
        assert_eq!(HwItem::new("k", Some("  ".into())).value, None);
        assert_eq!(HwItem::new("k", Some("".into())).value, None);
        assert_eq!(
            HwItem::new("k", Some("  ASUS ".into())).value,
            Some("ASUS".into())
        );
    }

    #[test]
    fn dmi_placeholders_are_rejected() {
        // 这些是主板厂没填真实值时留下的占位串，展示出来会被当成真型号
        for junk in [
            "System Product Name",
            "To Be Filled By O.E.M.",
            "Default string",
            "  Unknown  ",
            "00000000",
        ] {
            assert_eq!(
                HwItem::new("k", Some(junk.into())).value,
                None,
                "placeholder {junk} leaked through"
            );
        }
        // 真实值不受影响，包括含 0 但非全 0 的序列号
        assert_eq!(
            HwItem::new("k", Some("240436541701519".into())).value,
            Some("240436541701519".into())
        );
        assert_eq!(
            HwItem::new("k", Some("ROG STRIX B760-G".into())).value,
            Some("ROG STRIX B760-G".into())
        );
    }

    #[test]
    fn serials_are_flagged_sensitive() {
        assert!(HwItem::secret("k", Some("SN123".into())).sensitive);
        assert!(!HwItem::new("k", Some("SN123".into())).sensitive);
    }

    #[test]
    fn uptime_is_formatted() {
        let s = uptime();
        assert!(s.contains('d') && s.contains('h') && s.contains('m'), "{s}");
    }

    #[test]
    fn storage_enum_codes_map_to_names() {
        assert_eq!(media_name(4), Some("SSD"));
        assert_eq!(media_name(3), Some("HDD"));
        assert_eq!(bus_name(17), Some("NVMe"));
        assert_eq!(bus_name(11), Some("SATA"));
        // 未知码不猜
        assert_eq!(media_name(99), None);
        assert_eq!(bus_name(99), None);
    }

    #[test]
    fn health_codes_map_to_keys() {
        assert_eq!(health_key(0), "hw.disk.healthy");
        assert_eq!(health_key(1), "hw.disk.warning");
        // 未知状态归入 unhealthy 而非静默忽略：宁可误报也不漏报
        assert_eq!(health_key(7), "hw.disk.unhealthy");
    }

    #[test]
    fn interface_types_map_to_keys() {
        assert_eq!(if_type_key(6), Some("hw.net.ethernet"));
        assert_eq!(if_type_key(71), Some("hw.net.wifi"));
        assert_eq!(if_type_key(999), None);
    }

    /// 真机：虚拟适配器必须被挡掉，否则列表会被 Wi-Fi Direct / VPN 隧道淹没
    #[test]
    #[ignore = "hw: 需要真实网卡"]
    fn virtual_adapters_are_filtered_out() {
        let hub = WmiHub::new();
        let nets = collect_net(&hub);
        assert!(!nets.is_empty(), "no adapter at all");
        for g in &nets {
            let desc = g
                .items
                .iter()
                .find(|i| i.key == "hw.net.description")
                .and_then(|i| i.value.clone())
                .unwrap_or_default();
            for junk in ["Wi-Fi Direct", "Tunnel", "TAP-Win32", "Pseudo"] {
                assert!(
                    !desc.contains(junk),
                    "virtual adapter leaked through: {desc}"
                );
            }
        }
    }

    /// 手动诊断：打印本机实际采集到的全部字段与耗时。
    /// 换机器排查"某项显示 N/A"时先跑这个，能直接看出是 WMI 没返回还是解析丢了。
    ///   cargo test --lib hwinfo_dump -- --ignored --nocapture
    #[test]
    #[ignore = "hw: 手动诊断，打印本机硬件信息"]
    fn hwinfo_dump() {
        let t0 = std::time::Instant::now();
        let info = collect();
        let cost = t0.elapsed();
        let mut missing = 0;
        for (cat, groups) in [
            ("CPU", &info.cpu),
            ("MEMORY", &info.memory),
            ("GPU", &info.gpu),
            ("DISK", &info.disk),
            ("NETWORK", &info.network),
            ("BOARD", &info.board),
            ("SYSTEM", &info.system),
        ] {
            println!("=== {cat} ===");
            for g in groups {
                if !g.title.is_empty() {
                    println!("  [{}]", g.title);
                }
                for it in &g.items {
                    match &it.value {
                        Some(v) => println!("  {:<26} {}", it.key, v),
                        None => {
                            missing += 1;
                            println!("  {:<26} <MISSING>", it.key);
                        }
                    }
                }
            }
        }
        println!("
collect() took {cost:?}, {missing} field(s) unavailable");
    }

    /// 真机采集：需要 WMI 可用，CI runner 上不保证
    #[test]
    #[ignore = "hw: 需要真实 WMI 环境"]
    fn collect_returns_plausible_hardware() {
        let info = collect();
        assert!(!info.cpu.is_empty(), "no CPU group");
        assert!(!info.system.is_empty(), "no system group");
        let cpu_model = info.cpu[0]
            .items
            .iter()
            .find(|i| i.key == "hw.cpu.model")
            .and_then(|i| i.value.clone());
        assert!(cpu_model.is_some(), "CPU model missing");
        // 指令集探测走 CPUID，任何 x86_64 机器都不该为空
        let feats = info.cpu[0]
            .items
            .iter()
            .find(|i| i.key == "hw.cpu.features")
            .and_then(|i| i.value.clone());
        assert!(feats.unwrap_or_default().contains("SSE4.2"));
    }
}
