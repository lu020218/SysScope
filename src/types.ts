/** 与后端 Snapshot 序列化形状一一对应的类型定义 */

export interface CpuPerf {
  perf_pct: number;
  c1_pct: number;
  c2_pct: number;
  c3_pct: number;
}

export interface CpuSnapshot {
  total: number;
  per_core: number[];
  freq_mhz: number;
  temp_c: number | null;
  power_w: number | null;
  power_peak_w: number | null;
  voltage_v: number | null;
  core_clocks: number[];
  base_mhz: number;
  effective_mhz: number | null;
  boost: boolean;
  perf: CpuPerf | null;
}

export interface MemSnapshot {
  total: number;
  used: number;
  available: number;
  swap_total: number;
  swap_used: number;
  compression: number | null;
  commit_used: number;
  commit_limit: number;
  hard_faults_ps: number;
  page_faults_ps: number;
  standby_bytes: number;
  modified_bytes: number;
  mem_speed_mts: number;
  mem_modules: number;
  theo_bandwidth_gbps: number;
}

export interface GpuSnapshot {
  name: string;
  util_pct: number;
  vram_used: number;
  vram_total: number;
  temp_c: number | null;
  power_w: number | null;
  power_limit_w: number | null;
  core_mhz: number | null;
  mem_mhz: number | null;
  fan_pct: number | null;
  mem_ctrl_pct: number | null;
  enc_pct: number | null;
  dec_pct: number | null;
  pcie_rx_kbs: number | null;
  pcie_tx_kbs: number | null;
  throttle_thermal: boolean;
  throttle_power: boolean;
  temp_slowdown_c: number | null;
  hotspot_c: number | null;
  fan_rpm: number | null;
  vram_temp_c: number | null;
}

export interface FpsSnapshot {
  status: "ok" | "no_admin" | "failed";
  pid: number;
  process: string;
  fps: number;
  frame_time_ms: number;
  low_1pct_fps: number;
  low_01pct_fps: number;
  ft_p95_ms: number;
  ft_p99_ms: number;
  stutters: number;
  has_data: boolean;
}

export interface NetIface {
  name: string;
  down_bps: number;
  up_bps: number;
  total_rx: number;
  total_tx: number;
}

export interface PingStats {
  target: string;
  rtt_ms: number | null;
  avg_ms: number;
  jitter_ms: number;
  loss_pct: number;
  active: boolean;
}

export interface AdapterUtil {
  name: string;
  link_bps: number;
  util_pct: number;
}

export interface NetSnapshot {
  down_bps: number;
  up_bps: number;
  total_rx: number;
  total_tx: number;
  ifaces: NetIface[];
  ping: PingStats;
  tcp_established: number;
  tcp_time_wait: number;
  tcp_listen: number;
  udp_endpoints: number;
  retrans_ps: number;
  retrans_pct: number;
  adapters: AdapterUtil[];
}

export interface ProcNetStat {
  pid: number;
  name: string;
  down_bps: number;
  up_bps: number;
}

export interface DiskIo {
  name: string;
  active_pct: number;
  read_bps: number;
  write_bps: number;
  queue_len: number;
  read_iops: number;
  write_iops: number;
  read_ms: number | null;
  write_ms: number | null;
}

export interface VolumeInfo {
  mount: string;
  total: number;
  available: number;
}

export interface StorageSnapshot {
  disks: DiskIo[];
  volumes: VolumeInfo[];
}

export interface StorageTemp {
  name: string;
  temp: number | null;
  temp2: number | null;
  life: number | null;
  written_gb: number | null;
}

export interface ProcStat {
  pid: number;
  name: string;
  cpu_pct: number;
  mem: number;
  disk_bps: number;
}

export interface GpuProcStat {
  pid: number;
  name: string;
  gpu_pct: number;
  vram: number;
}

export interface ProcDetail {
  pid: number;
  cpu_time_ms: number;
  threads: number;
  handles: number;
  working_set: number;
  working_set_peak: number;
  private_bytes: number;
  page_faults: number;
  priority: string;
  affinity_mask: number;
  ok: boolean;
}

export interface Snapshot {
  ts: number;
  cpu: CpuSnapshot;
  mem: MemSnapshot;
  gpus: GpuSnapshot[];
  fps: FpsSnapshot;
  net: NetSnapshot;
  storage: StorageSnapshot;
  storage_temps: StorageTemp[];
  top_cpu: ProcStat[];
  top_mem: ProcStat[];
  top_net: ProcNetStat[];
  top_disk: ProcStat[];
  top_gpu: GpuProcStat[];
}

export interface StaticInfo {
  cpu_name: string;
  logical_cores: number;
  physical_cores: number | null;
  total_mem: number;
  os: string;
  hostname: string;
  core_classes: number[];
}

export interface RecStatus {
  active: boolean;
  session_id: number | null;
  started_at: number | null;
  samples: number;
}

export interface SessionMeta {
  id: number;
  started_at: number;
  ended_at: number | null;
  samples: number;
}
