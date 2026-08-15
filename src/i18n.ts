/**
 * 轻量 i18n：语言包静态打包（无运行时 fetch，避免首屏闪烁与 CSP 问题）。
 * 语言状态只存在于前端；后端仅托盘/提权弹窗少量原生文案自带一张小表。
 */

import en from "./locales/en";
import zh from "./locales/zh-CN";

export type Lang = "zh-CN" | "en";

/** zh-CN 是基准语言包，其余语言的 key 集合由 TS 在编译期校验（见 locales/en.ts） */
const DICTS: Record<Lang, Record<string, string>> = { "zh-CN": zh, en };
const BASE: Record<string, string> = zh;

const STORAGE_KEY = "sysscope-lang";

/** 跟随系统 UI 语言：WebView2 的 navigator.language 反映 Windows 显示语言 */
function detect(): Lang {
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

function load(): Lang {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "zh-CN" || saved === "en" ? saved : detect();
}

let lang: Lang = load();
let dict = DICTS[lang];

export function currentLang(): Lang {
  return lang;
}

/**
 * 取翻译；缺 key 时回退到 zh-CN，再缺则原样返回 key —— 宁可显示 key
 * 也不能让界面出现空白（缺翻译要看得见）。
 * vars 用 {name} 占位：t("toast.exported", { path })
 */
export function t(key: string, vars?: Record<string, string | number>): string {
  let s = dict[key] ?? BASE[key] ?? key;
  if (vars) {
    // split/join 而非 replaceAll：tsconfig target 为 ES2020，replaceAll 是 ES2021
    for (const [k, v] of Object.entries(vars)) {
      s = s.split(`{${k}}`).join(String(v));
    }
  }
  return s;
}

/**
 * 翻译静态标注：data-i18n 写 textContent，data-i18n-title 写 title。
 * 一律走 textContent 而非 innerHTML，与「不可信字符串不进 innerHTML」的约定一致。
 */
export function applyStatic(root: ParentNode = document) {
  for (const el of root.querySelectorAll<HTMLElement>("[data-i18n]")) {
    el.textContent = t(el.dataset.i18n!);
  }
  for (const el of root.querySelectorAll<HTMLElement>("[data-i18n-title]")) {
    el.title = t(el.dataset.i18nTitle!);
  }
  document.documentElement.lang = lang;
}

/**
 * 切换语言并重载页面。卡片是 build() 建一次 DOM、update() 刷值的结构，
 * 重建全部 DOM 的代价与风险高于重载；实时曲线缓冲（RingBuffers，纯内存）
 * 会重新开始，但录制中的会话由后端采样线程写 SQLite，不受影响。
 */
export function setLang(next: Lang) {
  if (next === lang) return;
  localStorage.setItem(STORAGE_KEY, next);
  location.reload();
}
