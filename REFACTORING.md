# SysScope 重构优化方案

> 版本：v1.0（2026-08-12）　依据：全项目代码评审结论
> 原则：每阶段结束时测试全绿、应用可运行、独立成 commit；先修风险后还债；小步提交可回滚。

## 阶段 0：版本管理基线（前置条件，≈10 分钟）

重构的安全网。没有它，后续任何一步都不可回滚。

1. `git init` + 合理的 `.gitignore`（target/、node_modules/、dist/、sensor-bridge/bin|obj/）
2. 首次提交：当前全部代码（功能完整、21 测试全绿的状态）
3. 此后每完成一个阶段/子项即提交一次

**验证**：`git log` 有基线提交；`git status` 干净。

---

## 阶段 1：安全与可靠性修复（高优先级，≈1 小时）

小 diff、高价值，先于架构调整完成。

### 1.1 HTML 报告脚本注入（🔴）
- **位置**：`report.rs:407`
- **修法**：`__DATA__` 嵌入前将 JSON 中的 `<` 替换为 `<`（`data.to_string().replace('<', "\\u003c")`）
- **测试**：新增用例——插入名为 `</script><script>alert(1)</script>` 的假进程名样本，断言导出 HTML 不含字面 `</script><script>`

### 1.2 采样线程守护（🔴）
- **位置**：`sampler.rs::spawn`
- **修法**：采样循环外包 `catch_unwind`；panic 时记录日志、向前端 emit `sampler-crashed` 事件、延迟 3s 后重建 `SamplerCtx` 继续；前端收到事件在顶栏显示红色警示条
- **测试**：注入一个 debug-only 的强制 panic 命令验证自愈路径（或人工验证后移除）

### 1.3 SQLite 并发（🟡）
- **修法**：`open_db` 统一执行 `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=3000;`
- **测试**：现有导出测试不回归即可（WAL 对现有逻辑透明）

### 1.4 CSP 收紧（🟡）
- **修法**：`tauri.conf.json` 的 csp 设为 `"default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:"`（uPlot/Vite 内联样式需要 unsafe-inline）
- **验证**：dev 与 build 两种模式下主窗口/悬浮窗功能完整、控制台无 CSP 报错

### 1.5 GPU 名称 innerHTML（🟡）
- **修法**：`buildGpuCards` 模板中标题占位空着，随后用 `textContent` 填入 `g.name`
- **验证**：GPU 卡片标题显示正常

### 1.6 单实例保护（🟡）
- **修法**：接入 `tauri-plugin-single-instance`，二次启动时唤起已有实例主窗口
- **验证**：连续启动两次 exe，只出现一个实例且主窗口被前置

---

## 阶段 2：后端架构重构——拆解 WMI 杂物间（≈2 小时）

**目标**：`disk.rs`（522 行、六个领域）按领域归位；模块名与职责一致；采集器间零耦合。

### 目标结构

```
src-tauri/src/
  wmi_hub.rs      # WmiHub：COM 初始化 + WMIConnection + query<T>() 原语（仅此一处碰 wmi crate）
  disk.rs         # 仅磁盘：DiskIo/卷空间/RawData 延迟差分（持 &WmiHub）
  mem_ext.rs      # MemExt：PerfOS_Memory + Win32_PhysicalMemory 静态信息
  cpu_perf.rs     # CpuPerf：ProcessorInformation + 基准频率；cpu_topo.rs 并入为子模块
  net_ext.rs      # NetExt：Tcpip 计数器 + 网卡利用率；netstat.rs（IpHelper 连接表）并入
  gpu_proc.rs     # 每进程 GPU/显存（2s 缓存）
  （其余模块不动：fps / gpu / ping / netproc / sensors / recorder / report / procdetail / etw_util）
```

### 实施步骤（每步一个 commit）

1. 新建 `wmi_hub.rs`，`DiskSampler` 内部改为持有 `WmiHub`，行为不变 → 测试全绿
2. 抽出 `mem_ext.rs`（含内存硬件静态查询），`SamplerCtx` 增持 `MemExtSampler`
3. 抽出 `cpu_perf.rs`（并入 cpu_topo）
4. 抽出 `net_ext.rs`（并入 netstat）
5. 抽出 `gpu_proc.rs`
6. `disk.rs` 只剩磁盘职责；各模块自带测试随迁

**约束**：`take_snapshot` 的输出结构（Snapshot 序列化形状）保持不变——前端零改动即为重构成功的判据。

---

## 阶段 3：前端模块化——拆解 main.ts（≈2.5 小时）

**目标**：main.ts（1269 行）拆为职责单一的模块；每张卡片独立文件；新增卡片不再改巨石。

### 目标结构

```
src/
  types.ts        # Snapshot 全家接口（与后端序列化形状一一对应）
  format.ts       # fmtBytes / fmtRate / fmtLinkSpeed / fmtCpuTime
  charts.ts       # makeChart 工厂 + 滚动缓冲区（RingBuffers 类：push/trim/window 切片）
  tabs.ts         # 通用标签页：initTabs(rootEl)，按 data-pane 约定自动接线（见 3.1）
  cards/
    cpu.ts        # CPU 卡片：build（P/E 分组、时钟格）+ update
    gpu.ts        # GPU 动态卡片
    mem.ts / disk.ts / net.ts / procs.ts
  modals/
    sessions.ts   # 会话列表与导出
    settings.ts   # 设置（阈值 + 自启 + 探测目标）
    procdetail.ts # 进程详情弹窗
  main.ts         # 引导：静态信息、listen 分发 update(s)、窗口控制、录制按钮（目标 <150 行）
```

- 每个卡片模块导出 `update(s: Snapshot, th: Thresholds)`；main.ts 的 metrics 监听只做分发
- 缓冲区管理（timestamps/cpuTotal/... 的 push/shift 对齐）收敛进 `RingBuffers`，消除六处手工同步

### 3.1 顺带归一三套标签页逻辑
- HTML 面板统一加 `data-pane` 属性；`initTabs` 扫描 `.tabs button[data-pane]` 与同卡片 `.tab-pane[data-pane]` 自动绑定
- CPU 特化版 / mem-disk-net 循环版 / GPU 每卡闭包版三处删除，替换为统一调用
- 各卡片记录激活面板，**隐藏面板跳过 DOM 更新**（顺带解决每秒空转重建）

**验证**：`npm run build` 通过；六卡片全功能人工冒烟（标签切换、弹窗、录制导出）。

---

## 阶段 4：质量统一（≈1.5 小时）

### 4.1 采样节拍防漂移
- `spawn` 循环改为目标时刻制：`next += interval; sleep(next - now)`；采样耗时超过间隔时跳拍并记录
- 顺带在 Snapshot 加 `sample_cost_ms` 字段（采样自身耗时），设置弹窗里可见——自我监控

### 4.2 测试分层（为 CI 铺路）
- 依赖真实硬件/管理员的测试统一标注 `#[ignore = "hw: 需要管理员/NVIDIA/联网"]`
- 本地全量：`cargo test -- --include-ignored`；CI 只跑纯逻辑测试
- README 记录两种跑法

### 4.3 告警阈值口径统一
- 硬编码阈值（磁盘活动 90%、硬页错误 200/s、延迟 100ms、丢包 2%、功耗墙 95%）全部收敛进 `thresholds.ts` 的常量区（一处可查）；是否开放到设置 UI 由后续需求决定

### 4.4 收尾
- `cargo clippy` 清零 warnings；`README` 更新模块结构图

---

## 明确不做（本轮范围外）

- 不引入前端框架（Vue/React）——当前规模模块化后 vanilla 完全够用
- 不重设计 recorder 库表（宽表 + 幂等迁移够用；等报告需求升级再说）
- 不做 i18n 抽取（一期中文的决策不变）
- 不动传感器桥（C# 侧结构简单清晰，无重构价值）

## 顺序与产出总览

| 阶段 | 内容 | 预估 | 产出 commit 数 |
|---|---|---|---|
| 0 | git 基线 | 10 分钟 | 1 |
| 1 | 6 项安全/可靠性修复 | 1 小时 | 6 |
| 2 | 后端 WMI 拆分 | 2 小时 | ~6 |
| 3 | 前端模块化 + 标签页归一 | 2.5 小时 | ~8 |
| 4 | 节拍/测试分层/阈值/clippy | 1.5 小时 | ~4 |

每阶段完成即是安全停点，可随时中断交付。
