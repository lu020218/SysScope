import { invoke } from "@tauri-apps/api/core";
import { $ } from "../format";
import { currentLang, setLang, type Lang } from "../i18n";
import {
  alertPrefs,
  DEFAULTS,
  fullSerials,
  saveAlertPrefs,
  saveThresholds,
  setFullSerials,
  thresholds,
} from "../thresholds";

/**
 * 把阈值与告警设置下发到后端。判定在采样线程进行，因此任何一处改动
 * （阈值、开关、持续时长）都要重新下发一次。
 */
export function pushAlertConfig() {
  const th = thresholds();
  const a = alertPrefs();
  void invoke("set_alert_config", {
    config: { ...th, enabled: a.enabled, dwellSecs: a.dwellSecs },
  }).catch(() => {});
}

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

  // 报告序列号脱敏开关（后端按此裁决，前端只负责传递）
  const serials = $("full-serials") as HTMLInputElement;
  serials.checked = fullSerials();
  serials.addEventListener("change", () => setFullSerials(serials.checked));

  for (const [id, key] of Object.entries(TH_INPUTS)) {
    $(id).addEventListener("change", (e) => {
      const v = Number((e.target as HTMLInputElement).value);
      if (Number.isFinite(v) && v > 0) {
        saveThresholds({ [key]: v });
        pushAlertConfig();
      }
    });
  }
  // 告警：开关与持续时长
  const alertOn = $("alert-enabled") as HTMLInputElement;
  const alertDwell = $("alert-dwell") as HTMLInputElement;
  const prefs = alertPrefs();
  alertOn.checked = prefs.enabled;
  alertDwell.value = String(prefs.dwellSecs);
  alertOn.addEventListener("change", () => {
    saveAlertPrefs({ enabled: alertOn.checked });
    pushAlertConfig();
  });
  alertDwell.addEventListener("change", () => {
    const v = Number(alertDwell.value);
    if (Number.isFinite(v) && v >= 0) {
      saveAlertPrefs({ dwellSecs: Math.round(v) });
      pushAlertConfig();
    }
  });

  $("th-reset").addEventListener("click", () => {
    saveThresholds({ ...DEFAULTS });
    pushAlertConfig();
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
