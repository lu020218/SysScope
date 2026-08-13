fn main() {
    // 自定义 Windows 应用清单：声明 requireAdministrator（FPS 的 ETW 内核会话
    // 与 CPU/GPU 温度的 LHM 内核驱动都必须提权）与 per-monitor DPI 感知。
    // 必须走 tauri_build 的 windows_attributes，直接用链接器 /MANIFESTINPUT
    // 会与 tauri_build 自己嵌入的清单资源冲突（CVT1100）。
    println!("cargo:rerun-if-changed=sysscope.manifest");
    let attrs = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("sysscope.manifest")),
    );
    tauri_build::try_build(attrs).expect("failed to run tauri build script");
}
