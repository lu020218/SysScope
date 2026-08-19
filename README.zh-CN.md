# SysScope

**面向 Windows 的专业级系统监控工具 —— 深度硬件指标、游戏悬浮窗、可导出报告。**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078d4)](#系统要求)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Rust%20%2B%20Tauri%202-orange)](https://tauri.app)

[English](README.md)

![SysScope 主面板](docs/images/dashboard.png)

SysScope 监控六类对象 —— CPU、GPU、内存、磁盘、网络、进程 —— 每一类都有两层视图：
概览页看实时曲线，详情页看出问题时真正需要的数字（热节流、功耗墙、磁盘队列、
TCP 重传、每进程显存占用……）。

单次采样约 **20 毫秒**，即使 0.5 秒档也只占用单核的几个百分点。

---

## 特性

**有深度，不只是仪表盘。** 多数轻量监控工具止步于"CPU 42%"，SysScope 还告诉你
*为什么*：封装功耗与每核频率、CPU 是否正在热降频、GPU 有没有撞功耗墙、磁盘队列
是否在堆积、内存压力是否已导致硬缺页。

| 监控对象 | 提供的指标 |
|---|---|
| **CPU** | 总占用与每核占用、P/E 核分组、封装功耗、核心电压、每核频率、有效频率、C-State 驻留、睿频状态、热节流标记 |
| **GPU** | 占用率、显存、核心/显存频率、温度 + 热点温度 + 显存温度、功耗与功耗上限、风扇 %/RPM、显存控制器负载、编解码引擎、PCIe 吞吐、**硬件级节流原因** |
| **内存** | 已用/可用/已缓存、备用与已修改页列表、提交内存与占比、硬/软页错误率、内存压缩、内存条频率与理论带宽 |
| **磁盘** | 每盘读写速率、IOPS、**亚毫秒级响应时间**、队列长度、剩余空间、SSD 健康度 / 累计写入 / 温度（SMART） |
| **网络** | 每网卡吞吐、ICMP 延迟与抖动丢包、TCP 连接状态分布、重传率、链路利用率 |
| **进程** | CPU / 内存 / 磁盘 / 网络 / **GPU** 五维 Top 5；点击任意行查看线程数、句柄数、工作集、私有提交、页错误、优先级与 CPU 亲和性 |

**硬件清单。** 独立面板列出这台机器到底是什么配置：CPU 步进、缓存大小、
微码版本与指令集；每条内存的型号、标称与实配速率及所在插槽；显卡的 VBIOS
与 PCIe 链路位宽；每块硬盘的介质类型、总线、固件与 SMART 健康度；物理网卡的
MAC、地址与链路速率；以及主板、BIOS 与系统版本号。一键复制为文本，方便提
issue 时贴出完整规格。

**游戏 FPS 悬浮窗。** 无边框置顶细条，自动跟随前台应用显示其帧率，并列出
CPU/GPU/内存/网络。帧数据来自 ETW（DXGI/D3D9 呈现事件），与 PresentMon 同源 ——
包含 1% / 0.1% Low、帧时间分位与卡顿计数。

![FPS 悬浮窗](docs/images/overlay.png)

**记录与报告。** 把监控会话录入 SQLite，然后导出为自包含的
**交互式 HTML 报告**、原始 **CSV/JSON**，或含均值/峰值/阈值超限统计的
**Markdown** 摘要。

---

## 下载

从 [Releases 页面](../../releases) 获取最新安装包。

`SysScope_x.y.z_x64_en-US.msi` —— 约 7 MB。

## 系统要求

- Windows 10 / 11，x64
- **管理员权限** —— 程序启动时会自动请求提权。FPS 采集需要 ETW 内核会话，
  温度读取需要内核驱动，二者都必须提权；其余指标在无权限时优雅降级。
- WebView2 运行时 —— Windows 11 及较新的 Windows 10 已内置
- NVIDIA 显卡可获得完整指标（通过 NVML）；AMD / Intel 显卡回退到占用率与显存

界面提供简体中文与英文两种语言，首次启动跟随 Windows 显示语言，可在
「设置 → 界面语言」中切换。导出的报告使用导出时的界面语言；CSV 与 JSON
的列名和字段名固定为英文，以免影响下游脚本解析。

报告导出到 `文档\SysScope\reports`，其中会附带本机硬件清单；序列号与 MAC
默认只保留后 4 位 —— 报告是拿来分享的，而这些值能唯一标识你的机器。确需完整值时
可在「设置 → 隐私」中开启。

---

## 从源码构建

需要 [Rust](https://rustup.rs)（MSVC 工具链）与 Node.js 20+。

```bash
npm install
npm run tauri dev     # 开发运行
npm run tauri build   # 打包安装程序
```

传感器桥（`sensor-bridge/`，用 NativeAOT 编译的 C# DLL，封装 LibreHardwareMonitor）
**已作为预编译 DLL 提交到仓库**，因此只改 Rust 或前端代码无需额外工具链。
只有修改桥本身才需要 .NET 9 SDK 与 VS C++ 工具链：

```bash
cd sensor-bridge && dotnet publish -c Release -r win-x64
```

产物复制到 `src-tauri/resources/sysscope_sensors.dll`。若链接器找不到 `vswhere`，
请在 VS 开发者命令行中执行，或把
`C:\Program Files (x86)\Microsoft Visual Studio\Installer` 加入 `PATH`。

### 测试

```bash
cd src-tauri && cargo test                        # 41 项纯逻辑测试，CI 可跑
cd src-tauri && cargo test -- --include-ignored   # 追加 11 项需真实硬件的测试
node scripts/check-i18n.mjs                       # 校验翻译 key 是否齐整
```

硬件层标注了 `#[ignore]`，因为它需要管理员权限、GPU 和活动桌面会话。

发布构建前请跑冒烟测试 —— 它验证程序**确实在工作**，而不只是进程存在
（历史上它抓到过多个仅查进程状态发现不了的回归）：

```bash
powershell -ExecutionPolicy Bypass -File scripts/smoke-test.ps1
```

---

## 实现要点

采集在单个采样线程上进行，每拍向前端发送一个 `Snapshot`。每项指标都选用
最省且最准的数据源：

| 数据源 | 用途 |
|---|---|
| **PDH** 性能计数器 | 磁盘 I/O、内存内部状态、CPU 性能与 C-State、TCP 与网卡统计、每进程 GPU |
| **NVML** | NVIDIA GPU 全量遥测 |
| **ETW** | 帧时间（DXGI/D3D9）与每进程网络字节数 |
| **LibreHardwareMonitor**（经 NativeAOT C ABI 桥） | 温度、CPU 功耗/电压、每核频率、SMART |
| **sysinfo / IpHelper / Win32** | 进程表、连接表、CPU 拓扑 |

关于 PDH 的一点经验：早期版本通过 WMI 查询 `Win32_PerfFormattedData_*`，
单拍成本高达 **1.7 秒** —— 因为每个这类查询都会阻塞等待一个内部采样窗口。
改用单一 PDH 查询句柄后，单拍降到 **20 毫秒**。如果你也在写 Windows 监控程序，
**不要在循环里轮询格式化的 WMI 计数器**。

源码结构见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

---

## 参与贡献

欢迎提交 Issue 与 Pull Request。报告 Bug 时附上冒烟测试输出和截图会很有帮助。
另外，多数采集器只能在真实硬件上验证，所以指标看起来不对时，请说明你的
CPU / GPU 型号。

## 许可证

[MIT](LICENSE) © lu020218

第三方组件（最重要的是 MPL-2.0 的 LibreHardwareMonitor）列于 [NOTICE](NOTICE)。
