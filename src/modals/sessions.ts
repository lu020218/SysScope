import { invoke } from "@tauri-apps/api/core";
import { $ } from "../format";
import { applyStatic, currentLang, t } from "../i18n";
import { ensureLoaded, forReport } from "./hwinfo";
import { fullSerials } from "../thresholds";
import type { RecStatus, SessionMeta } from "../types";

let recording = false;
let recStartedAt = 0;
/** 点击代次：用于丢弃点击前已在途的状态轮询结果，避免陈旧值回翻按钮 */
let clickSeq = 0;
/** 点击后的期望状态：后端未达成前，轮询结果不回写按钮（防"落定前闪烁"） */
let pendingTarget: boolean | null = null;
let pendingSince = 0;

function renderRecordBtn(samples?: number) {
  const btn = $("record-btn");
  btn.classList.toggle("recording", recording);
  if (recording) {
    const secs = Math.max(0, Math.floor((Date.now() - recStartedAt) / 1000));
    const mm = String(Math.floor(secs / 60)).padStart(2, "0");
    const ss = String(secs % 60).padStart(2, "0");
    btn.textContent = `■ ${mm}:${ss}${samples != null ? ` · ${samples}` : ""}`;
  } else {
    btn.textContent = t("rec.startBtn");
  }
}

async function syncRecStatus() {
  const seq = clickSeq;
  let st: RecStatus;
  try {
    st = await invoke<RecStatus>("recording_status");
  } catch {
    // 启动早期后端 state 可能尚未注册；轮询会自动重试，静默跳过本轮
    return;
  }
  if (seq !== clickSeq) return; // 期间发生过点击，丢弃陈旧结果
  if (pendingTarget != null) {
    // 后端已达成期望状态或等待超时（3s 兜底）前，忽略中间态
    if (st.active === pendingTarget || Date.now() - pendingSince > 3000) {
      pendingTarget = null;
    } else {
      return;
    }
  }
  recording = st.active;
  recStartedAt = st.started_at ?? recStartedAt;
  renderRecordBtn(st.samples);
}

function fmtTime(ms: number): string {
  return new Date(ms).toLocaleString(currentLang(), { hour12: false });
}

function fmtDur(a: number, b: number | null): string {
  if (b == null) return t("sessions.inProgress");
  const s = Math.max(0, Math.floor((b - a) / 1000));
  return t("sessions.duration", {
    h: Math.floor(s / 3600),
    m: Math.floor((s % 3600) / 60),
    s: s % 60,
  });
}

async function refreshSessions() {
  const list = $("sessions-list");
  const toast = $("export-toast");
  const sessions = await invoke<SessionMeta[]>("list_sessions");
  if (sessions.length === 0) {
    list.innerHTML = `<div class="sessions-empty"></div>`;
    list.firstElementChild!.textContent = t("sessions.empty");
    return;
  }
  list.innerHTML = "";
  for (const s of sessions) {
    const row = document.createElement("div");
    row.className = "session-row";
    row.innerHTML = `
      <div class="session-info">
        <span>#${s.id} · ${fmtTime(s.started_at)}</span>
        <small>${fmtDur(s.started_at, s.ended_at)} · ${t("sessions.samples", { n: s.samples })}</small>
      </div>
      <div class="session-actions">
        <button data-fmt="html">HTML</button>
        <button data-fmt="csv">CSV</button>
        <button data-fmt="json">JSON</button>
        <button data-fmt="md" data-i18n="sessions.md">摘要</button>
        <button data-fmt="__del" class="danger" data-i18n="sessions.delete">删除</button>
      </div>`;
    applyStatic(row);
    row.querySelector(".session-actions")!.addEventListener("click", async (e) => {
      const btn = (e.target as HTMLElement).closest("button");
      if (!btn) return;
      const fmt = btn.dataset.fmt!;
      try {
        if (fmt === "__del") {
          await invoke("delete_session", { sessionId: s.id });
          await refreshSessions();
          return;
        }
        // 报告带上机器规格：没有配置信息的性能报告价值会打折
        await ensureLoaded();
        const path = await invoke<string>("export_report", {
          sessionId: s.id,
          format: fmt,
          lang: currentLang(),
          hardware: forReport() ?? [],
          fullSerials: fullSerials(),
        });
        toast.textContent = t("sessions.exported", { path });
        toast.classList.remove("hidden", "error");
        toast.onclick = () => invoke("open_in_folder", { path });
      } catch (err) {
        toast.textContent = t("sessions.exportFailed", { err: String(err) });
        toast.classList.remove("hidden");
        toast.classList.add("error");
        toast.onclick = null;
      }
    });
    list.appendChild(row);
  }
}

export async function init() {
  await syncRecStatus();
  // 持续轮询，以后端状态为唯一事实来源（采样线程 ≤200ms 内响应开/停请求）
  setInterval(() => void syncRecStatus(), 1000);

  $("record-btn").addEventListener("click", async () => {
    clickSeq += 1;
    const target = !recording;
    pendingTarget = target;
    pendingSince = Date.now();
    await invoke(target ? "start_recording" : "stop_recording");
    // 乐观渲染；后端达成期望状态前，轮询不回写按钮
    recording = target;
    recStartedAt = Date.now();
    renderRecordBtn();
  });

  const modal = $("sessions-modal");
  $("sessions-btn").addEventListener("click", async () => {
    $("export-toast").classList.add("hidden");
    modal.classList.remove("hidden");
    await refreshSessions();
  });
  $("open-reports").addEventListener("click", async () => {
    const dir = await invoke<string>("open_reports_dir");
    const toast = $("export-toast");
    toast.textContent = t("sessions.reportsDir", { dir });
    toast.classList.remove("hidden", "error");
    toast.onclick = null;
  });
  $("modal-close").addEventListener("click", () => modal.classList.add("hidden"));
  modal.addEventListener("click", (e) => {
    if (e.target === modal) modal.classList.add("hidden");
  });
}
