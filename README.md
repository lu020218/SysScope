# SysScope

Windows 桌面系统状态实时监控工具（Rust + Tauri）。

实时监控 CPU、GPU、内存、FPS 等关键指标，支持会话记录与监控报告导出。
完整需求见 [REQUIREMENTS.md](REQUIREMENTS.md)。

## 当前进度

- [x] M1 骨架：Tauri 应用框架 + CPU / 内存实时采集与曲线展示
- [x] M2 GPU：NVML + WMI 双路 GPU 指标接入（占用 / 显存 / 温度 / 功耗）
- [x] M3 FPS：ETW 帧率采集（DXGI/D3D9 Present）+ 前台窗口自动跟踪，需管理员权限
- [x] 网络监控：各接口上/下行速率 + 累计流量（过滤回环与 Npcap 镜像接口）
- [x] FPS 悬浮窗（OSD）：无边框置顶小窗展示 FPS + CPU/GPU/内存简略值，可拖动、可开关
- [x] CPU 温度：LibreHardwareMonitor 经 NativeAOT 编译为原生 DLL，Rust FFI 进程内调用（需管理员权限）
- [x] M4 记录与报告：会话记录、SQLite 落盘、HTML/CSV/JSON/Markdown 报告导出
- [x] M5 打磨：托盘常驻（关闭到托盘）、开机自启（静默启动）、置顶/紧凑模式、
      设置面板（告警阈值配置）、WMI 虚拟适配器过滤
- [x] 专业化一批：磁盘 I/O（活动率/读写速率/队列/分区空间/盘温）、CPU 功耗与热节流标注、
      GPU 频率/风扇/功耗墙、FPS 0.1% Low + P95/P99 + 卡顿计数、CPU/内存 Top-5 进程；
      录制与报告同步扩展（库表 v2 幂等迁移）
- [x] 专业化二批：提交内存 + 硬页错误率（WMI PerfOS）、ICMP 延迟探测（RTT/抖动/丢包，
      目标可配置）、每进程网络流量 Top-5（ETW Kernel-Network）；
      ETW 会话名加 PID 后缀（多实例/测试并存互不干扰，启动时清理孤儿会话）
- [x] CPU 深化：每核频率与核心电压（LHM）、P/E 核分组利用率（拓扑 API）、
      有效频率与 C1-C3 驻留（性能计数器）、峰值功耗/睿频状态/频率会话统计；
      CPU 卡片改为四标签页（概览/核心/频率/电源）
- [x] GPU 深化：显存控制器负载、编解码引擎负载、PCIe 吞吐（低频采样）、
      硬件级热节流/功耗墙标志（NVML throttle reasons）、降频阈值温度、
      Hotspot/风扇 RPM/显存温度（LHM/NVAPI）；GPU 卡片双标签页 + 节流脉冲徽章
- [x] 内存深化：已缓存/备用/已修改页列表、总页错误率、内存压缩（MemCompression
      工作集）、提交占比、内存频率与条数（Win32_PhysicalMemory）、理论带宽推算；
      内存卡片双标签页
- [x] 磁盘深化：读写 IOPS、平均响应时间（PerfRawData 差分，规避格式化计数器
      整数截断）、队列展示、SSD 健康/累计写入 TBW/控制器温度（LHM SMART，
      受 Intel RST 拦截时显示 N/A 并提示）；磁盘卡片双标签页
- [x] 网络深化：TCP 连接分状态计数（已建立/TIME_WAIT/监听）、UDP 端点数
      （IpHelper 连接表）、TCP 重传率（Tcpip 计数器）、各网卡链路速率与
      利用率（NetworkInterface）；网络卡片双标签页
- [x] 进程深化：Top 列表增磁盘 I/O 列（sysinfo disk_usage）与 GPU/显存列
      （WMI per-PID 计数器 2s 缓存）；点击进程行弹详情面板——累计 CPU 时间、
      线程/句柄数、工作集/峰值、私有提交、页错误、优先级、CPU 亲和性位图
- [x] 无边框主窗口：自绘标题栏（顶栏即拖动区，双击最大化），Fluent 风格
      最小化/最大化/关闭三键（关闭走托盘），窗口边缘保留原生拖拽缩放

## 开发

依赖：Rust（MSVC）、Node.js 20+

```bash
npm install
npm run tauri dev     # 开发运行
npm run tauri build   # 发布构建
```

后端测试：

```bash
cd src-tauri && cargo test
```

## 结构

- `src/` — 前端（Vite + TypeScript + uPlot 图表）
- `src-tauri/src/sampler.rs` — 指标采集（sysinfo，1s 间隔可调，事件推送）
- `src-tauri/src/gpu.rs` — GPU 指标（NVML 优先，WMI 性能计数器兜底）
- `src-tauri/src/fps.rs` — FPS 采集（ETW 订阅 DXGI/D3D9 Present 事件，需管理员权限）
- `src-tauri/src/sensors.rs` — 温度传感器 FFI（加载 sysscope_sensors.dll）
- `src-tauri/src/recorder.rs` — 会话记录（SQLite，保留最近 30 个会话）
- `src-tauri/src/report.rs` — 报告导出（HTML 内嵌 uPlot / CSV / JSON / Markdown）
- 数据位置：`%APPDATA%/com.luhaishan.sysscope/sysscope.db`，报告在同目录 `reports/` 下
- `sensor-bridge/` — C# 桥接层（LibreHardwareMonitor，NativeAOT 编译为原生 DLL）

重建传感器 DLL（需 .NET 9 SDK + VS C++ 工具链）：

```bash
cd sensor-bridge && dotnet publish -c Release -r win-x64
```

产物复制到 `src-tauri/resources/sysscope_sensors.dll`（已随仓库提供预编译版本）。
若报 vswhere/link.exe 找不到，在 VS 开发者命令行中执行，或将
`C:\Program Files (x86)\Microsoft Visual Studio\Installer` 加入 PATH。
- `src-tauri/src/lib.rs` — Tauri 应用入口与命令注册
