mod alerts;
mod cpu_perf;
mod disk;
mod elevate;
mod etw_util;
mod fps;
mod gpu;
mod gpu_proc;
mod hwinfo;
mod i18n;
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
use tauri::{AppHandle, Emitter, Manager};

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

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        set_webview_memory_level(app, false);
    }
}

/// 主窗口隐藏到托盘时把 WebView2 降到低内存态（释放渲染缓存等，
/// 常驻内存显著下降），恢复显示时切回正常。失败静默忽略——
/// 该接口需较新的 WebView2 运行时，缺失时仅仅是没有优化而已。
fn set_webview_memory_level(app: &AppHandle, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    // 必须用 webview2-com 同版本（0.61）的 Interface trait；项目主 windows
    // 依赖是 0.58，两套 COM 类型不通用
    use windows_core::Interface;

    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    let _ = w.with_webview(move |webview| unsafe {
        if let Ok(core) = webview.controller().CoreWebView2() {
            if let Ok(v19) = core.cast::<ICoreWebView2_19>() {
                let _ = v19.SetMemoryUsageTargetLevel(level);
            }
        }
    });
}

/// 托盘菜单项句柄：语言切换时就地改文本，避免重建整个托盘
struct TrayItems {
    show: MenuItem<tauri::Wry>,
    record: MenuItem<tauri::Wry>,
    overlay: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

/// 前端切换语言后同步原生托盘（用户可能覆盖了系统语言）
#[tauri::command]
fn set_language(app: AppHandle, lang: String) {
    let lang = i18n::Lang::parse(&lang);
    // 告警通知等原生文案也要跟随，不止托盘
    i18n::set_current(lang);
    let Some(items) = app.try_state::<Arc<TrayItems>>() else {
        return;
    };
    let _ = items.show.set_text(i18n::tr(lang, "tray.show"));
    let _ = items.record.set_text(i18n::tr(lang, "tray.record"));
    let _ = items.overlay.set_text(i18n::tr(lang, "tray.overlay"));
    let _ = items.quit.set_text(i18n::tr(lang, "tray.quit"));
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_tooltip(Some(i18n::tr(lang, "tray.tooltip")));
    }
    // 悬浮窗是独立 WebView，主窗口重载影响不到它，广播让它自己重载
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.emit("lang-changed", ());
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    // 初始跟随系统语言；前端就绪后若用户另有选择，会经 set_language 覆盖
    let lang = i18n::sys_lang();
    let show = MenuItem::with_id(app, "show", i18n::tr(lang, "tray.show"), true, None::<&str>)?;
    let record = MenuItem::with_id(
        app,
        "record",
        i18n::tr(lang, "tray.record"),
        true,
        None::<&str>,
    )?;
    let overlay = MenuItem::with_id(
        app,
        "overlay",
        i18n::tr(lang, "tray.overlay"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", i18n::tr(lang, "tray.quit"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &record, &overlay, &quit])?;
    app.manage(Arc::new(TrayItems {
        show: show.clone(),
        record: record.clone(),
        overlay: overlay.clone(),
        quit: quit.clone(),
    }));

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip(i18n::tr(lang, "tray.tooltip"))
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
    // 必须在初始化 Tauri（含单实例插件）之前完成：未提权时本进程只负责
    // 拉起提权实例然后退出，不应注册任何全局状态
    if !elevate::ensure_elevated() {
        return;
    }
    tauri::Builder::default()
        // 单实例：二次启动（如自启 + 手动双击）时唤起既有实例的主窗口
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            sampler::get_static_info,
            sampler::set_sample_interval,
            toggle_overlay,
            window_control,
            set_main_on_top,
            set_language,
            alerts::set_alert_config,
            hwinfo::hardware_info,
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

            // 状态注册必须先于任何耗时操作：Tauri 在 setup 之前就已创建窗口
            // 并开始加载前端，前端可能在 setup 仍在跑时就发起命令调用。
            // 若此时 state 未注册，命令会以 "state not managed" 失败，
            // 前端初始化链随之中断（面板停留在"加载系统信息…"）。
            app.manage(ctl.clone());
            app.manage(recorder::DbPath(db_path.clone()));
            app.manage(recorder::ReportsDir(reports_dir.clone()));

            recorder::prune_old_sessions(&db_path);

            // 一次性迁移旧 AppData/reports（加密目录）下的历史报告
            if let Some(old) = db_path.parent().map(|p| p.join("reports")) {
                recorder::migrate_legacy_reports(&old, &reports_dir);
            }
            println!("[sysscope] reports dir: {}", reports_dir.display());

            sampler::spawn(app.handle().clone(), ctl, db_path);

            setup_tray(app)?;

            // --minimized：静默启动到托盘（供任务计划程序等外部方式调用）
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
                set_webview_memory_level(app.handle(), true);
            }
            Ok(())
        })
        // 关闭主窗口时最小化到托盘而非退出；退出走托盘菜单
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                    // 隐藏到托盘后释放 WebView2 渲染内存
                    set_webview_memory_level(window.app_handle(), true);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
