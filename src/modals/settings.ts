import { invoke } from "@tauri-apps/api/core";
import { $ } from "../format";
import { currentLang, setLang, type Lang } from "../i18n";
import { DEFAULTS, saveThresholds, thresholds } from "../thresholds";

const TH_INPUTS: Record<string, keyof typeof DEFAULTS> = {
  "th-cpu": "cpu",
  "th-mem": "mem",
  "th-gpu": "gpu",
  "th-cpu-temp": "cpuTemp",
  "th-gpu-temp": "gpuTemp",
};

const PING_KEY = "sysscope-ping-target";

function fillSettings() {
  const th = thresholds();
  for (const [id, key] of Object.entries(TH_INPUTS)) {
    ($(id) as HTMLInputElement).value = String(th[key]);
  }
}

export function init() {
  const modal = $("settings-modal");

  $("settings-btn").addEventListener("click", () => {
    fillSettings();
    modal.classList.remove("hidden");
  });
  $("settings-close").addEventListener("click", () => modal.classList.add("hidden"));
  modal.addEventListener("click", (e) => {
    if (e.target === modal) modal.classList.add("hidden");
  });

  // 界面语言（切换后重载页面，见 i18n.setLang）
  const langSel = $("lang-select") as HTMLSelectElement;
  langSel.value = currentLang();
  langSel.addEventListener("change", () => setLang(langSel.value as Lang));

  for (const [id, key] of Object.entries(TH_INPUTS)) {
    $(id).addEventListener("change", (e) => {
      const v = Number((e.target as HTMLInputElement).value);
      if (Number.isFinite(v) && v > 0) saveThresholds({ [key]: v });
    });
  }
  $("th-reset").addEventListener("click", () => {
    saveThresholds({ ...DEFAULTS });
    fillSettings();
  });

  // 延迟探测目标（持久化并在启动时恢复）
  const savedTarget = localStorage.getItem(PING_KEY);
  if (savedTarget) {
    ($("ping-target") as HTMLInputElement).value = savedTarget;
    void invoke("set_ping_target", { target: savedTarget });
  }
  $("ping-target").addEventListener("change", async (e) => {
    const v = (e.target as HTMLInputElement).value.trim();
    if (v) {
      localStorage.setItem(PING_KEY, v);
      await invoke("set_ping_target", { target: v });
    }
  });
}
