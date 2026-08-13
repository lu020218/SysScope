mod cpu_perf;
mod disk;
mod etw_util;
mod fps;
mod gpu;
mod gpu_proc;
mod mem_ext;
mod net_ext;
mod netproc;
mod pdh;
mod ping;
mod procdetail;
mod recorder;
mod report;
mod sampler;
mod sensors;
mod wmi_hub;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

/// 切换 FPS 悬浮窗显隐，返回切换后的可见状态
#[tauri::command]
fn toggle_overlay(app: AppHandle) -> bool {
    let Some(w) = app.get_webview_window("overlay") else {
        return false;
    };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        false
    } else {
        let _ = w.show();
        true
    }
}

/// 无边框主窗口的自绘标题栏控制
#[tauri::command]
fn window_control(app: AppHandle, action: String) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    match action.as_str() {
        "min" => {
            let _ = w.minimize();
        }
        "max" => {
            if w.is_maximized().unwrap_or(false) {
                let _ = w.unmaximize();
            } else {
                let _ = w.maximize();
            }
        }
        // close 触发 CloseRequested → 统一走隐藏到托盘逻辑
        "close" => {
            let _ = w.close();
        }
        _ => {}
    }
}

/// 主窗口置顶开关
#[tauri::command]
fn set_main_on_top(app: AppHandle, on: bool) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(on);
    }
}

#[tauri::command]
fn set_autostart(app: AppHandle, enable: bool) -> Result<bool, String> {
    let m = app.autolaunch();
    if enable {
        m.enable().map_err(|e| e.to_string())?;
    } else {
        m.disable().map_err(|e| e.to_string())?;
    }
    m.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示面板", true, None::<&str>)?;
    let record = MenuItem::with_id(app, "record", "开始 / 停止记录", true, None::<&str>)?;
    let overlay = MenuItem::with_id(app, "overlay", "显示 / 隐藏悬浮窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &record, &overlay, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("SysScope 系统监控")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "record" => {
                let ctl = app.state::<Arc<recorder::RecorderCtl>>();
                let cur = ctl.requested.load(Ordering::Relaxed);
                ctl.requested.store(!cur, Ordering::Relaxed);
            }
            "overlay" => {
                toggle_overlay(app.clone());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例：二次启动（如自启 + 手动双击）时唤起既有实例的主窗口
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .invoke_handler(tauri::generate_handler![
            sampler::get_static_info,
            sampler::set_sample_interval,
            toggle_overlay,
            window_control,
            set_main_on_top,
            set_autostart,
            get_autostart,
            ping::set_ping_target,
            procdetail::process_detail,
            recorder::start_recording,
            recorder::stop_recording,
            recorder::recording_status,
            recorder::list_sessions,
            recorder::delete_session,
            recorder::export_report,
            recorder::open_in_folder,
            recorder::open_reports_dir
        ])
        .setup(|app| {
            let ctl = Arc::new(recorder::RecorderCtl::default());
            let db_path = app
                .path()
                .app_data_dir()
                .expect("app data dir unavailable")
                .join("sysscope.db");
            recorder::prune_old_sessions(&db_path);

            // 报告导出目录：优先用户文档目录（易访问、非 EFS 加密），
            // 取不到时回退到 DB 同级的 reports/
            let reports_dir = app
                .path()
                .document_dir()
                .map(|d| d.join("SysScope").join("reports"))
                .unwrap_or_else(|_| {
                    db_path
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join("reports")
                });
            // 一次性迁移旧 AppData/reports（加密目录）下的历史报告
            if let Some(old) = db_path.parent().map(|p| p.join("reports")) {
                recorder::migrate_legacy_reports(&old, &reports_dir);
            }
            println!("[sysscope] reports dir: {}", reports_dir.display());

            app.manage(ctl.clone());
            app.manage(recorder::DbPath(db_path.clone()));
            app.manage(recorder::ReportsDir(reports_dir));
            sampler::spawn(app.handle().clone(), ctl, db_path);

            setup_tray(app)?;

            // 开机自启（--minimized）时静默启动到托盘
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        // 关闭主窗口时最小化到托盘而非退出；退出走托盘菜单
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
