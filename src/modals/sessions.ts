import { invoke } from "@tauri-apps/api/core";
import { $ } from "../format";
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
    btn.textContent = "● 记录";
  }
}

async function syncRecStatus() {
  const seq = clickSeq;
  const st = await invoke<RecStatus>("recording_status");
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
  return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

function fmtDur(a: number, b: number | null): string {
  if (b == null) return "进行中";
  const s = Math.max(0, Math.floor((b - a) / 1000));
  return `${Math.floor(s / 3600)}时${Math.floor((s % 3600) / 60)}分${s % 60}秒`;
}

async function refreshSessions() {
  const list = $("sessions-list");
  const toast = $("export-toast");
  const sessions = await invoke<SessionMeta[]>("list_sessions");
  if (sessions.length === 0) {
    list.innerHTML = `<div class="sessions-empty">暂无会话，点击顶栏"● 记录"开始录制</div>`;
    return;
  }
  list.innerHTML = "";
  for (const s of sessions) {
    const row = document.createElement("div");
    row.className = "session-row";
    row.innerHTML = `
      <div class="session-info">
        <span>#${s.id} · ${fmtTime(s.started_at)}</span>
        <small>${fmtDur(s.started_at, s.ended_at)} · ${s.samples} 个采样</small>
      </div>
      <div class="session-actions">
        <button data-fmt="html">HTML</button>
        <button data-fmt="csv">CSV</button>
        <button data-fmt="json">JSON</button>
        <button data-fmt="md">摘要</button>
        <button data-fmt="__del" class="danger">删除</button>
      </div>`;
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
        const path = await invoke<string>("export_report", {
          sessionId: s.id,
          format: fmt,
        });
        toast.textContent = `已导出：${path}（点击定位文件）`;
        toast.classList.remove("hidden", "error");
        toast.onclick = () => invoke("open_in_folder", { path });
      } catch (err) {
        toast.textContent = `导出失败：${err}`;
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
    toast.textContent = `报告目录：${dir}`;
    toast.classList.remove("hidden", "error");
    toast.onclick = null;
  });
  $("modal-close").addEventListener("click", () => modal.classList.add("hidden"));
  modal.addEventListener("click", (e) => {
    if (e.target === modal) modal.classList.add("hidden");
  });
}
