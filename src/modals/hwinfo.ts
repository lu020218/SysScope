import { invoke } from "@tauri-apps/api/core";
import { $ } from "../format";
import { t } from "../i18n";

interface HwItem {
  key: string;
  value: string | null;
  sensitive?: boolean;
}
interface HwGroup {
  title: string;
  items: HwItem[];
}
interface HwInfo {
  cpu: HwGroup[];
  memory: HwGroup[];
  gpu: HwGroup[];
  disk: HwGroup[];
  network: HwGroup[];
  board: HwGroup[];
  system: HwGroup[];
}

type Category = keyof HwInfo;

const CATEGORIES: [Category, string][] = [
  ["cpu", "hw.cat.cpu"],
  ["memory", "hw.cat.memory"],
  ["gpu", "hw.cat.gpu"],
  ["disk", "hw.cat.disk"],
  ["network", "hw.cat.network"],
  ["board", "hw.cat.board"],
  ["system", "hw.cat.system"],
];

let data: HwInfo | null = null;
let active: Category = "cpu";

/**
 * 后端的 value 要么是设备原文（"NVMe"、"16 GB"），要么是一个 i18n key
 * （"hw.disk.healthy"、"hw.net.up"）。统一过一遍 t()：命中就翻译，
 * 命不中原样返回 —— 设备原文不会长得像点分小写的 key，不存在误伤。
 */
function displayValue(v: string | null): string {
  return v == null ? "N/A" : t(v);
}

function renderNav() {
  const nav = $("hw-nav");
  nav.innerHTML = "";
  for (const [cat, key] of CATEGORIES) {
    const btn = document.createElement("button");
    btn.textContent = t(key);
    btn.classList.toggle("active", cat === active);
    // 该分类无数据时置灰但保留，让用户知道这台机器上采不到
    btn.disabled = (data?.[cat]?.length ?? 0) === 0;
    btn.addEventListener("click", () => {
      active = cat;
      renderNav();
      renderContent();
    });
    nav.appendChild(btn);
  }
}

function renderContent() {
  const wrap = $("hw-content");
  wrap.innerHTML = "";
  const groups = data?.[active] ?? [];
  if (groups.length === 0) {
    const empty = document.createElement("div");
    empty.className = "sessions-empty";
    empty.textContent = t("hw.empty");
    wrap.appendChild(empty);
    return;
  }
  for (const g of groups) {
    const block = document.createElement("div");
    block.className = "dd-block";
    if (g.title) {
      const name = document.createElement("div");
      name.className = "dd-name";
      // 型号名来自驱动与固件（外部字符串），走 textContent 避免注入
      name.textContent = g.title;
      block.appendChild(name);
    }
    const rows = document.createElement("div");
    rows.className = "kv-rows kv-2col";
    for (const it of g.items) {
      const row = document.createElement("div");
      row.className = "kv";
      const k = document.createElement("span");
      k.textContent = t(it.key);
      const v = document.createElement("b");
      const text = displayValue(it.value);
      v.textContent = text;
      if (it.value == null) v.classList.add("na");
      // 长值（IP 列表、指令集列表）在半宽格子里会从中间断行，
      // 把 IPv6 地址劈成两半 —— 让它独占整行
      if (text.length > 40) row.classList.add("kv-wide");
      row.append(k, v);
      rows.appendChild(row);
    }
    block.appendChild(rows);
    wrap.appendChild(block);
  }
}

/** 全部分类拼成纯文本，供贴进 issue / 论坛 */
function asText(): string {
  if (!data) return "";
  const lines: string[] = [];
  for (const [cat, key] of CATEGORIES) {
    const groups = data[cat];
    if (groups.length === 0) continue;
    lines.push(`## ${t(key)}`);
    for (const g of groups) {
      if (g.title) lines.push(`### ${g.title}`);
      for (const it of g.items) {
        lines.push(`${t(it.key)}: ${displayValue(it.value)}`);
      }
    }
    lines.push("");
  }
  return lines.join("\n");
}

/**
 * 供报告导出使用：标签与值都已翻译好（硬件标签只存在于前端语言包），
 * 但保留 sensitive 原样 —— 是否脱敏由后端按开关统一裁决，
 * 这样"未显式开启就绝不写进文件"是后端不变式，而非各调用方自觉。
 * 返回 null 表示尚未打开过硬件信息弹窗，此时报告不含硬件节。
 */
export function forReport() {
  if (!data) return null;
  return CATEGORIES.map(([cat, key]) => ({
    title: t(key),
    blocks: data![cat].map((g) => ({
      title: g.title,
      rows: g.items.map((it) => ({
        label: t(it.key),
        value: displayValue(it.value),
        sensitive: it.sensitive ?? false,
      })),
    })),
  })).filter((sec) => sec.blocks.length > 0);
}

/** 报告导出前确保硬件信息已就绪（用户可能从未打开过弹窗） */
export async function ensureLoaded() {
  if (data) return;
  try {
    data = await invoke<HwInfo>("hardware_info");
  } catch {
    // 采集失败不应阻断报告导出，仅让报告不含硬件节
  }
}

export function init() {
  const modal = $("hw-modal");
  const close = () => modal.classList.add("hidden");

  $("hw-btn").addEventListener("click", async () => {
    modal.classList.remove("hidden");
    if (!data) {
      // 首次采集在后端约 300ms（NVML 初始化占大头），之后命中缓存瞬回
      $("hw-content").textContent = t("hw.loading");
      try {
        data = await invoke<HwInfo>("hardware_info");
      } catch (e) {
        $("hw-content").textContent = t("hw.failed", { err: String(e) });
        return;
      }
    }
    renderNav();
    renderContent();
  });

  $("hw-copy").addEventListener("click", async () => {
    const btn = $("hw-copy");
    await navigator.clipboard.writeText(asText());
    btn.textContent = t("hw.copied");
    setTimeout(() => (btn.textContent = t("hw.copy")), 1500);
  });

  $("hw-close").addEventListener("click", close);
  modal.addEventListener("click", (e) => {
    if (e.target === modal) close();
  });
}
