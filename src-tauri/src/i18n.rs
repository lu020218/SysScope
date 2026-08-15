//! 后端只有极少量原生文案需要多语言（托盘菜单、提权弹窗），
//! 用一张静态表即可，不引任何 i18n 依赖。语言状态的权威在前端；
//! 后端默认跟随系统 UI 语言，前端通过 `set_language` 覆盖。
//!
//! 例外：`elevate.rs` 的提权失败弹窗在 Tauri 启动前就要弹，那时既无
//! WebView 也无用户配置可读，只能用系统语言。

use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    ZhCn,
    En,
}

impl Lang {
    /// 解析前端传来的标记；未知值回退英文
    pub fn parse(s: &str) -> Self {
        if s.to_ascii_lowercase().starts_with("zh") {
            Lang::ZhCn
        } else {
            Lang::En
        }
    }
}

/// Windows 显示语言；LANGID 低 10 位是主语言 ID，0x04 = 中文
pub fn sys_lang() -> Lang {
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid & 0x3ff == 0x04 {
        Lang::ZhCn
    } else {
        Lang::En
    }
}

/// 返回值的生命周期绑定到 key，未命中时可以直接把 key 借出去（无需泄漏）
pub fn tr(lang: Lang, key: &str) -> &str {
    match (lang, key) {
        (Lang::ZhCn, "tray.show") => "显示面板",
        (Lang::ZhCn, "tray.record") => "开始 / 停止记录",
        (Lang::ZhCn, "tray.overlay") => "显示 / 隐藏悬浮窗",
        (Lang::ZhCn, "tray.quit") => "退出",
        (Lang::ZhCn, "tray.tooltip") => "SysScope 系统监控",
        (Lang::ZhCn, "elevate.title") => "SysScope",
        (Lang::ZhCn, "elevate.body") => {
            "SysScope 需要管理员权限才能采集 FPS 与温度等指标。\n\n请在 UAC 提示中允许，或右键选择“以管理员身份运行”。"
        }

        (_, "tray.show") => "Show dashboard",
        (_, "tray.record") => "Start / stop recording",
        (_, "tray.overlay") => "Show / hide overlay",
        (_, "tray.quit") => "Quit",
        (_, "tray.tooltip") => "SysScope system monitor",
        (_, "elevate.title") => "SysScope",
        (_, "elevate.body") => {
            "SysScope needs administrator rights to collect FPS and temperature data.\n\nAllow it in the UAC prompt, or right-click the app and choose \"Run as administrator\"."
        }

        // 缺 key 时返回 key 本身：宁可显示 key 也不要空白菜单项
        (_, other) => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_translated_in_both_languages() {
        for key in [
            "tray.show",
            "tray.record",
            "tray.overlay",
            "tray.quit",
            "tray.tooltip",
            "elevate.title",
            "elevate.body",
        ] {
            for lang in [Lang::ZhCn, Lang::En] {
                assert_ne!(tr(lang, key), key, "{key} missing for {lang:?}");
            }
        }
    }

    #[test]
    fn parse_falls_back_to_english() {
        assert_eq!(Lang::parse("zh-CN"), Lang::ZhCn);
        assert_eq!(Lang::parse("zh"), Lang::ZhCn);
        assert_eq!(Lang::parse("en"), Lang::En);
        assert_eq!(Lang::parse("klingon"), Lang::En);
    }
}
