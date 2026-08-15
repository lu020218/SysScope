//! 启动时自我提权。
//!
//! FPS 采集（ETW 内核会话）与 CPU/GPU 温度（LHM 内核驱动）必须管理员权限。
//! 清单本可直接写 requireAdministrator，但那样 MSI 安装完成页的"启动程序"
//! 会失效——该动作以非提权令牌调用 CreateProcess，无法拉起
//! requireAdministrator 的程序（ERROR_ELEVATION_REQUIRED），且 MSI 用
//! Return="asyncNoWait" 不检查返回值，失败完全静默。
//!
//! 故清单声明 asInvoker，由程序自己在初始化 Tauri 之前检测并用
//! ShellExecute("runas") 重启自身。所有启动路径（MSI、双击、命令行、
//! 任务计划）行为一致，且提权发生在 Tauri 初始化前，不会与单实例插件冲突。

use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// 当前进程是否已提权
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// 以管理员身份重启自身（透传原命令行参数）。
/// 返回 true 表示已成功发起提权实例，调用方应立即退出当前进程。
pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    // 参数里可能含空格，逐个加引号
    let args: Vec<String> = std::env::args()
        .skip(1)
        .map(|a| format!("\"{a}\""))
        .collect();
    let params = args.join(" ");

    let exe_w = wide(exe.as_os_str());
    let params_w = wide(std::ffi::OsStr::new(&params));
    let verb_w = wide(std::ffi::OsStr::new("runas"));

    unsafe {
        let h = ShellExecuteW(
            None,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            if params.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params_w.as_ptr())
            },
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        // ShellExecuteW 返回值 > 32 表示成功；用户在 UAC 弹窗点"否"返回
        // SE_ERR_ACCESSDENIED(5)，此时不重启、由调用方决定如何提示
        h.0 as usize > 32
    }
}

/// 确保以管理员权限运行：已提权则继续；未提权则拉起提权实例并要求退出。
/// 返回 false 表示当前进程应当立即结束。
pub fn ensure_elevated() -> bool {
    if is_elevated() {
        return true;
    }
    if relaunch_as_admin() {
        return false; // 提权实例已启动，本进程退出
    }
    // 用户拒绝提权：提示后退出，避免以残缺功能运行造成误解。
    // 这里只能用系统语言 —— 此刻 WebView 尚未创建，读不到用户的语言选择
    let lang = crate::i18n::sys_lang();
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
        let text = wide(std::ffi::OsStr::new(crate::i18n::tr(lang, "elevate.body")));
        let caption = wide(std::ffi::OsStr::new(crate::i18n::tr(lang, "elevate.title")));
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_check_runs() {
        // 仅验证调用不 panic；具体值取决于测试进程的令牌
        let e = is_elevated();
        println!("current process elevated: {e}");
    }
}
