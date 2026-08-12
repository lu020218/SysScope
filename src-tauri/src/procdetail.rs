use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessAffinityMask, GetProcessHandleCount, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

/// 单进程详情（按需查询，前端弹窗每秒调用一次）
#[derive(Serialize, Clone, Default)]
pub struct ProcDetail {
    pub pid: u32,
    /// 累计 CPU 时间（毫秒，内核 + 用户态）
    pub cpu_time_ms: u64,
    pub threads: u32,
    pub handles: u32,
    /// 工作集 / 峰值工作集 / 私有提交（字节）
    pub working_set: u64,
    pub working_set_peak: u64,
    pub private_bytes: u64,
    /// 累计页错误次数
    pub page_faults: u64,
    pub priority: String,
    /// 进程亲和性掩码（可用逻辑核位图）
    pub affinity_mask: u64,
    pub ok: bool,
}

fn filetime_ms(ft: &FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64) / 10_000
}

fn priority_name(class: u32) -> String {
    match class {
        0x40 => "低".into(),
        0x4000 => "低于正常".into(),
        0x20 => "正常".into(),
        0x8000 => "高于正常".into(),
        0x80 => "高".into(),
        0x100 => "实时".into(),
        other => format!("0x{other:X}"),
    }
}

/// 统计目标进程的线程数（Toolhelp 全量线程快照过滤）
fn thread_count(pid: u32) -> u32 {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return 0;
        };
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut count = 0u32;
        if Thread32First(snap, &mut entry).is_ok() {
            loop {
                if entry.th32OwnerProcessID == pid {
                    count += 1;
                }
                if Thread32Next(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        count
    }
}

#[tauri::command]
pub fn process_detail(pid: u32) -> ProcDetail {
    unsafe {
        let Ok(h): Result<HANDLE, _> = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
        else {
            return ProcDetail {
                pid,
                ok: false,
                ..Default::default()
            };
        };

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let cpu_time_ms = GetProcessTimes(h, &mut creation, &mut exit, &mut kernel, &mut user)
            .map(|_| filetime_ms(&kernel) + filetime_ms(&user))
            .unwrap_or(0);

        let mut handles = 0u32;
        let _ = GetProcessHandleCount(h, &mut handles);

        let mut mem = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let _ = GetProcessMemoryInfo(h, &mut mem, mem.cb);

        let priority = priority_name(GetPriorityClass(h));

        let mut proc_mask = 0usize;
        let mut sys_mask = 0usize;
        let _ = GetProcessAffinityMask(h, &mut proc_mask, &mut sys_mask);

        let _ = CloseHandle(h);

        ProcDetail {
            pid,
            cpu_time_ms,
            threads: thread_count(pid),
            handles,
            working_set: mem.WorkingSetSize as u64,
            working_set_peak: mem.PeakWorkingSetSize as u64,
            private_bytes: mem.PagefileUsage as u64,
            page_faults: mem.PageFaultCount as u64,
            priority,
            affinity_mask: proc_mask as u64,
            ok: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_of_own_process_is_plausible() {
        let d = process_detail(std::process::id());
        println!(
            "self: cpu_time={}ms threads={} handles={} ws={}MB private={}MB faults={} prio={} affinity={:b}",
            d.cpu_time_ms,
            d.threads,
            d.handles,
            d.working_set >> 20,
            d.private_bytes >> 20,
            d.page_faults,
            d.priority,
            d.affinity_mask,
        );
        assert!(d.ok);
        assert!(d.threads > 0);
        assert!(d.handles > 0);
        assert!(d.working_set > 0);
        assert!(d.private_bytes > 0);
        assert!(d.affinity_mask > 0);
    }

    #[test]
    fn detail_of_missing_process_degrades() {
        let d = process_detail(4_294_000_000);
        assert!(!d.ok);
    }
}
