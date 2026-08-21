using System;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;
using LibreHardwareMonitor.Hardware;

namespace SysScope;

/// <summary>
/// LibreHardwareMonitor 的 C ABI 桥接层，经 NativeAOT 编译为 sysscope_sensors.dll，
/// 由 Rust 侧 FFI 加载。仅启用 CPU 传感器（温度）。
/// 注意：底层依赖内核驱动（PawnIO/WinRing0），init 需要管理员权限。
/// </summary>
public static class Bridge
{
    private static Computer? _computer;
    private static readonly object Lock = new();
    private static string _lastError = "";

    /// <summary>取最近一次错误信息（UTF-8 写入 buf），返回写入字节数；buf 为空时返回所需长度</summary>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_last_error")]
    public static unsafe int LastError(byte* buf, int len)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(_lastError);
        if (buf == null || len <= 0)
        {
            return bytes.Length;
        }
        int n = Math.Min(bytes.Length, len - 1);
        for (int i = 0; i < n; i++)
        {
            buf[i] = bytes[i];
        }
        buf[n] = 0;
        return n;
    }

    /// <returns>0 成功；-1 失败（通常为权限不足或驱动加载失败）</returns>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_sensors_init")]
    public static int Init()
    {
        try
        {
            lock (Lock)
            {
                if (_computer != null)
                {
                    return 0;
                }
                var computer = new Computer
                {
                    IsCpuEnabled = true,
                    IsStorageEnabled = true,
                    IsGpuEnabled = true,
                    // 主板域提供机箱风扇转速与 VRM / 芯片组温度 —— 回答
                    // "CPU 降频是不是因为供电过热"这类问题，其余数据源给不出。
                    // 这些传感器挂在主板的 SubHardware（SuperIO 芯片）下，
                    // 不在 Motherboard 硬件本身上。
                    IsMotherboardEnabled = true,
                };
                computer.Open();
                _computer = computer;
            }
            return 0;
        }
        catch (Exception e)
        {
            _lastError = e.ToString();
            return -1;
        }
    }

    /// <returns>CPU 温度（摄氏度）；无可用传感器时返回 -1</returns>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_cpu_temp")]
    public static float CpuTemp()
    {
        try
        {
            lock (Lock)
            {
                if (_computer == null)
                {
                    return -1f;
                }
                // 优先取代表整体的传感器（AMD Tctl/Tdie、Intel CPU Package），
                // 否则退化为全部温度传感器的最大值
                float preferred = -1f;
                float max = -1f;
                foreach (var hw in _computer.Hardware)
                {
                    if (hw.HardwareType != HardwareType.Cpu)
                    {
                        continue;
                    }
                    hw.Update();
                    foreach (var sensor in hw.Sensors)
                    {
                        if (sensor.SensorType != SensorType.Temperature || sensor.Value is not { } v)
                        {
                            continue;
                        }
                        var value = (float)v;
                        if (sensor.Name.Contains("Tctl") || sensor.Name.Contains("Package"))
                        {
                            preferred = Math.Max(preferred, value);
                        }
                        max = Math.Max(max, value);
                    }
                }
                return preferred >= 0 ? preferred : max;
            }
        }
        catch
        {
            return -1f;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "sysscope_sensors_shutdown")]
    public static void Shutdown()
    {
        lock (Lock)
        {
            _computer?.Close();
            _computer = null;
        }
    }

    private static void AppendJsonString(StringBuilder sb, string s)
    {
        sb.Append('"');
        foreach (var c in s)
        {
            if (c == '"' || c == '\\')
            {
                sb.Append('\\');
                sb.Append(c);
            }
            else if (c < 0x20)
            {
                sb.Append(' ');
            }
            else
            {
                sb.Append(c);
            }
        }
        sb.Append('"');
    }

    private static void AppendNum(StringBuilder sb, float v)
    {
        sb.Append(v.ToString("0.##", CultureInfo.InvariantCulture));
    }

    /// <summary>
    /// 诊断用：逐个硬件域测量 Update() 耗时（毫秒）。
    ///
    /// 加这个是因为"哪个域慢"靠猜连错两次：先以为是 SuperIO（实测 1.2ms），
    /// 再以为是 SMART（拆走后仍慢）。与其继续猜，不如让桥自己报出来。
    ///
    /// 结构：[{"name":"...","type":"Cpu","ms":12.3}]
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_timing_json")]
    public static unsafe int TimingJson(byte* buf, int len)
    {
        try
        {
            lock (Lock)
            {
                if (_computer == null || buf == null || len <= 0)
                {
                    return -1;
                }
                var sb = new StringBuilder(1024);
                sb.Append('[');
                bool first = true;
                foreach (var hw in _computer.Hardware)
                {
                    var sw = System.Diagnostics.Stopwatch.StartNew();
                    hw.Update();
                    sw.Stop();
                    if (!first)
                    {
                        sb.Append(',');
                    }
                    first = false;
                    sb.Append("{\"name\":");
                    AppendJsonString(sb, hw.Name);
                    sb.Append(",\"type\":");
                    AppendJsonString(sb, hw.HardwareType.ToString());
                    sb.Append(",\"ms\":");
                    AppendNum(sb, (float)sw.Elapsed.TotalMilliseconds);
                    sb.Append(",\"sensors\":");
                    AppendNum(sb, hw.Sensors.Length);
                    sb.Append('}');
                }
                sb.Append(']');

                var bytes = Encoding.UTF8.GetBytes(sb.ToString());
                if (bytes.Length >= len)
                {
                    return -1;
                }
                for (int i = 0; i < bytes.Length; i++)
                {
                    buf[i] = bytes[i];
                }
                buf[bytes.Length] = 0;
                return bytes.Length;
            }
        }
        catch (Exception e)
        {
            _lastError = e.ToString();
            return -1;
        }
    }

    /// <summary>
    /// 硬盘 SMART（温度、健康度、累计写入），与每拍调用的 SensorsJson 分开导出。
    ///
    /// 分开的依据是实测：LHM 的 Storage.Update() 走 SMART IOCTL，本机两块 NVMe
    /// 合计约 390ms 中位数，占满一整拍的 95%。而 SMART 数据变化极慢 ——
    /// 温度以秒计、健康度以月计、累计写入以天计 —— 每拍读取纯属浪费。
    /// 调用方按十秒级轮询即可。
    ///
    /// 结构：[{"name":"...","temp":42.0,"temp2":50.0,"life":98.0,"written_gb":1234.5}]
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_storage_json")]
    public static unsafe int StorageJson(byte* buf, int len)
    {
        try
        {
            lock (Lock)
            {
                if (_computer == null || buf == null || len <= 0)
                {
                    return -1;
                }
                var storage = new StringBuilder();
                bool firstDisk = true;
                foreach (var hw in _computer.Hardware)
                {
                    if (hw.HardwareType != HardwareType.Storage)
                    {
                        continue;
                    }

                        hw.Update();
                        float dTemp = -1f, dTemp2 = -1f, dLife = -1f, dWritten = -1f;
                        foreach (var s in hw.Sensors)
                        {
                            if (s.Value is not { } v)
                            {
                                continue;
                            }
                            var value = (float)v;
                            if (s.SensorType == SensorType.Temperature)
                            {
                                // "Temperature" 为复合温度；"Temperature 2" 常为控制器
                                if (s.Name == "Temperature" || dTemp < 0)
                                {
                                    if (s.Name.Contains('2'))
                                    {
                                        dTemp2 = value;
                                    }
                                    else
                                    {
                                        dTemp = value;
                                    }
                                }
                                else if (s.Name.Contains('2'))
                                {
                                    dTemp2 = value;
                                }
                            }
                            else if (s.SensorType == SensorType.Level &&
                                     s.Name.Contains("Remaining Life"))
                            {
                                dLife = value;
                            }
                            else if (s.SensorType == SensorType.Data &&
                                     s.Name.Contains("Data Written"))
                            {
                                dWritten = value; // GB
                            }
                        }
                        if (dTemp >= 0 || dLife >= 0 || dWritten >= 0)
                        {
                            if (!firstDisk)
                            {
                                storage.Append(',');
                            }
                            firstDisk = false;
                            storage.Append("{\"name\":");
                            AppendJsonString(storage, hw.Name);
                            if (dTemp >= 0)
                            {
                                storage.Append(",\"temp\":");
                                AppendNum(storage, dTemp);
                            }
                            if (dTemp2 >= 0)
                            {
                                storage.Append(",\"temp2\":");
                                AppendNum(storage, dTemp2);
                            }
                            if (dLife >= 0)
                            {
                                storage.Append(",\"life\":");
                                AppendNum(storage, dLife);
                            }
                            if (dWritten >= 0)
                            {
                                storage.Append(",\"written_gb\":");
                                AppendNum(storage, dWritten);
                            }
                            storage.Append('}');
                        }
                    
                }
                var sb = new StringBuilder(512);
                sb.Append('[').Append(storage).Append(']');
                var bytes = Encoding.UTF8.GetBytes(sb.ToString());
                if (bytes.Length >= len)
                {
                    return -1;
                }
                for (int i = 0; i < bytes.Length; i++)
                {
                    buf[i] = bytes[i];
                }
                buf[bytes.Length] = 0;
                return bytes.Length;
            }
        }
        catch (Exception e)
        {
            _lastError = e.ToString();
            return -1;
        }
    }

    /// <summary>
    /// 主板 SuperIO 传感器（风扇转速、VRM/芯片组温度），与每拍调用的
    /// SensorsJson 分开导出。
    ///
    /// 分开的原因：SuperIO 读取走 LPC/EC 端口 I/O，比其余传感器慢一个量级，
    /// 而风扇转速与主板温度本身变化缓慢 —— 每拍读它没有任何收益，只有代价。
    /// 调用方按秒级而非按拍轮询即可。
    ///
    /// 结构：{"name":"...","fans":[{"name":"..","rpm":1200}],"temps":[{"name":"..","value":42}]}
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_board_json")]
    public static unsafe int BoardJson(byte* buf, int len)
    {
        try
        {
            lock (Lock)
            {
                if (_computer == null || buf == null || len <= 0)
                {
                    return -1;
                }
                var fans = new StringBuilder();
                var temps = new StringBuilder();
                bool firstFan = true, firstTemp = true;
                string boardName = "";

                foreach (var hw in _computer.Hardware)
                {
                    if (hw.HardwareType != HardwareType.Motherboard)
                    {
                        continue;
                    }
                    boardName = hw.Name;
                    // 主板本身不带传感器，读数都在 SuperIO 子硬件上
                    foreach (var sub in hw.SubHardware)
                    {
                        sub.Update();
                        foreach (var s in sub.Sensors)
                        {
                            if (s.Value is not { } v)
                            {
                                continue;
                            }
                            var value = (float)v;
                            // 未接风扇的接口报 0 转，列出来只是噪音
                            if (s.SensorType == SensorType.Fan && value > 0f)
                            {
                                if (!firstFan)
                                {
                                    fans.Append(',');
                                }
                                firstFan = false;
                                fans.Append("{\"name\":");
                                AppendJsonString(fans, s.Name);
                                fans.Append(",\"rpm\":");
                                AppendNum(fans, value);
                                fans.Append('}');
                            }
                            // 未接线的温度接口会报出物理上不可能的值（实测某接口
                            // 常驻 4°C）。运行中的主板传感器不可能低于室温，
                            // 原样显示会让人以为某处真的只有 4 度。
                            // 上限同理挡掉 SuperIO 偶发的满量程读数。
                            else if (s.SensorType == SensorType.Temperature &&
                                     value >= 10f && value <= 125f)
                            {
                                if (!firstTemp)
                                {
                                    temps.Append(',');
                                }
                                firstTemp = false;
                                temps.Append("{\"name\":");
                                AppendJsonString(temps, s.Name);
                                temps.Append(",\"value\":");
                                AppendNum(temps, value);
                                temps.Append('}');
                            }
                        }
                    }
                }

                var sb = new StringBuilder(512);
                sb.Append("{\"name\":");
                AppendJsonString(sb, boardName);
                sb.Append(",\"fans\":[").Append(fans).Append("],");
                sb.Append("\"temps\":[").Append(temps).Append("]}");

                var bytes = Encoding.UTF8.GetBytes(sb.ToString());
                if (bytes.Length >= len)
                {
                    return -1;
                }
                for (int i = 0; i < bytes.Length; i++)
                {
                    buf[i] = bytes[i];
                }
                buf[bytes.Length] = 0;
                return bytes.Length;
            }
        }
        catch (Exception e)
        {
            _lastError = e.ToString();
            return -1;
        }
    }

    /// <summary>
    /// 全量传感器读数（UTF-8 JSON 写入 buf），返回写入字节数；失败返回 -1。
    /// 结构：{"cpu_temp":55.0,"cpu_power":45.2,"storage":[{"name":"...","temp":42.0}]}
    /// 手工拼 JSON 以保证 NativeAOT 兼容（反射序列化在 AOT 下不可用）。
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "sysscope_sensors_json")]
    public static unsafe int SensorsJson(byte* buf, int len)
    {
        try
        {
            lock (Lock)
            {
                if (_computer == null || buf == null || len <= 0)
                {
                    return -1;
                }
                float cpuTempPreferred = -1f, cpuTempMax = -1f, cpuPower = -1f;
                float cpuVoltagePreferred = -1f, cpuVoltageAny = -1f;
                float gpuHotspot = -1f, gpuFanRpm = -1f, gpuVramTemp = -1f;
                bool gpuDone = false;
                var coreClocks = new System.Collections.Generic.List<float>();
                var sb = new StringBuilder(512);
                sb.Append('{');

                foreach (var hw in _computer.Hardware)
                {
                    if (hw.HardwareType == HardwareType.Cpu)
                    {
                        hw.Update();
                        foreach (var s in hw.Sensors)
                        {
                            if (s.Value is not { } v)
                            {
                                continue;
                            }
                            var value = (float)v;
                            if (s.SensorType == SensorType.Temperature)
                            {
                                if (s.Name.Contains("Tctl") || s.Name.Contains("Package"))
                                {
                                    cpuTempPreferred = Math.Max(cpuTempPreferred, value);
                                }
                                cpuTempMax = Math.Max(cpuTempMax, value);
                            }
                            else if (s.SensorType == SensorType.Power &&
                                     (s.Name.Contains("Package") || s.Name == "CPU Package"))
                            {
                                cpuPower = value;
                            }
                            else if (s.SensorType == SensorType.Clock &&
                                     s.Name.StartsWith("CPU Core") && !s.Name.Contains("Bus"))
                            {
                                coreClocks.Add(value);
                            }
                            else if (s.SensorType == SensorType.Voltage)
                            {
                                if (s.Name == "CPU Core" || s.Name.Contains("Core (SVI2"))
                                {
                                    cpuVoltagePreferred = value;
                                }
                                if (cpuVoltageAny < 0)
                                {
                                    cpuVoltageAny = value;
                                }
                            }
                        }
                    }
                    else if (hw.HardwareType == HardwareType.GpuNvidia ||
                             hw.HardwareType == HardwareType.GpuAmd ||
                             hw.HardwareType == HardwareType.GpuIntel)
                    {
                        // 仅取第一块独显（与 NVML 主 GPU 对应）
                        if (gpuDone)
                        {
                            continue;
                        }
                        gpuDone = true;
                        hw.Update();
                        foreach (var s in hw.Sensors)
                        {
                            if (s.Value is not { } v)
                            {
                                continue;
                            }
                            var value = (float)v;
                            if (s.SensorType == SensorType.Temperature)
                            {
                                if (s.Name.Contains("Hot Spot"))
                                {
                                    gpuHotspot = value;
                                }
                                else if (s.Name.Contains("Memory"))
                                {
                                    gpuVramTemp = value;
                                }
                            }
                            else if (s.SensorType == SensorType.Fan && gpuFanRpm < 0)
                            {
                                gpuFanRpm = value;
                            }
                        }
                    }

                }

                var cpuTemp = cpuTempPreferred >= 0 ? cpuTempPreferred : cpuTempMax;
                if (cpuTemp >= 0)
                {
                    sb.Append("\"cpu_temp\":");
                    AppendNum(sb, cpuTemp);
                    sb.Append(',');
                }
                if (cpuPower >= 0)
                {
                    sb.Append("\"cpu_power\":");
                    AppendNum(sb, cpuPower);
                    sb.Append(',');
                }
                var cpuVoltage = cpuVoltagePreferred >= 0 ? cpuVoltagePreferred : cpuVoltageAny;
                if (cpuVoltage >= 0)
                {
                    sb.Append("\"cpu_voltage\":");
                    sb.Append(cpuVoltage.ToString("0.###", CultureInfo.InvariantCulture));
                    sb.Append(',');
                }
                if (gpuHotspot >= 0)
                {
                    sb.Append("\"gpu_hotspot\":");
                    AppendNum(sb, gpuHotspot);
                    sb.Append(',');
                }
                if (gpuFanRpm >= 0)
                {
                    sb.Append("\"gpu_fan_rpm\":");
                    AppendNum(sb, gpuFanRpm);
                    sb.Append(',');
                }
                if (gpuVramTemp >= 0)
                {
                    sb.Append("\"gpu_vram_temp\":");
                    AppendNum(sb, gpuVramTemp);
                    sb.Append(',');
                }
                sb.Append("\"core_clocks\":[");
                for (int i = 0; i < coreClocks.Count; i++)
                {
                    if (i > 0)
                    {
                        sb.Append(',');
                    }
                    AppendNum(sb, coreClocks[i]);
                }
                sb.Append("],");
                sb.Append("\"ok\":1}");

                var bytes = Encoding.UTF8.GetBytes(sb.ToString());
                if (bytes.Length >= len)
                {
                    return -1;
                }
                for (int i = 0; i < bytes.Length; i++)
                {
                    buf[i] = bytes[i];
                }
                buf[bytes.Length] = 0;
                return bytes.Length;
            }
        }
        catch (Exception e)
        {
            _lastError = e.ToString();
            return -1;
        }
    }
}
