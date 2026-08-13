use sysinfo::{Pid, ProcessesToUpdate, System};

/// 本进程专属的 ETW 会话名（带 PID 后缀，避免测试进程与运行中的应用互相
/// 停掉对方的同名会话）
pub fn session_name(prefix: &str) -> String {
    format!("{prefix}_{}", std::process::id())
}

/// 清理属主进程已退出的残留会话（异常退出时 ETW 会话不会自动销毁）。
///
/// 注意：会话创建本身不依赖清理完成——同名冲突已由 PID 后缀规避，
/// 清理只是回收孤儿。因此启动路径上应异步执行（见 spawn_cleanup），
/// 避免 logman 全量枚举（数百毫秒）阻塞采集器初始化。
pub fn cleanup_stale_sessions(prefixes: &[&str]) {
    let Ok(out) = std::process::Command::new("logman")
        .args(["query", "-ets"])
        .output()
    else {
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sys = System::new();
    let self_pid = std::process::id();
    for line in text.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        // 同一次枚举服务所有前缀，避免重复调用 logman
        let Some(pid_str) = prefixes
            .iter()
            .find_map(|p| name.strip_prefix(*p)?.strip_prefix('_'))
        else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        if sys.process(Pid::from_u32(pid)).is_none() {
            let _ = std::process::Command::new("logman")
                .args(["stop", name, "-ets"])
                .output();
        }
    }
}

/// 后台清理孤儿会话，不阻塞启动路径
pub fn spawn_cleanup(prefixes: &'static [&'static str]) {
    std::thread::spawn(move || cleanup_stale_sessions(prefixes));
}
