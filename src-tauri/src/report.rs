use crate::i18n::{tr, tr_fmt, Lang};
use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

const UPLOT_JS: &str = include_str!("../templates/uPlot.iife.min.js");
const UPLOT_CSS: &str = include_str!("../templates/uPlot.min.css");
const HTML_TEMPLATE: &str = include_str!("../templates/report.html");

// ---------- 硬件信息（由前端随导出请求传入） ----------
//
// 硬件标签（hw.cpu.model 等）只存在于前端语言包。让后端也持有一份意味着
// 85 条中英文案要在两处维护；改为由前端传入已翻译好的 label + value，
// 报告这边只负责排版。
//
// 但脱敏必须留在后端：前端传的是原始值加 sensitive 标记，是否展示完整
// 序列号由此处统一裁决 —— 这样"未显式开启就绝不写进文件"是一条后端不变式，
// 而不依赖每个调用方自觉。
#[derive(Deserialize, Serialize, Clone)]
pub struct HwRow {
    pub label: String,
    pub value: String,
    /// 机器指纹（序列号、MAC）：未开启完整显示时脱敏
    #[serde(default)]
    pub sensitive: bool,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct HwBlock {
    pub title: String,
    pub rows: Vec<HwRow>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct HwSection {
    pub title: String,
    pub blocks: Vec<HwBlock>,
}

/// 只保留末 4 位。足以在多块同型号硬盘之间区分，又不构成可追溯的机器指纹。
fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "*".repeat(chars.len().max(1));
    }
    format!("****{}", chars[chars.len() - 4..].iter().collect::<String>())
}

/// 按开关决定是否脱敏，返回可直接写进文件的副本
fn apply_masking(hw: &[HwSection], full_serials: bool) -> Vec<HwSection> {
    if full_serials {
        return hw.to_vec();
    }
    hw.iter()
        .map(|sec| HwSection {
            title: sec.title.clone(),
            blocks: sec
                .blocks
                .iter()
                .map(|b| HwBlock {
                    title: b.title.clone(),
                    rows: b
                        .rows
                        .iter()
                        .map(|r| HwRow {
                            label: r.label.clone(),
                            value: if r.sensitive {
                                mask(&r.value)
                            } else {
                                r.value.clone()
                            },
                            sensitive: r.sensitive,
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

#[derive(Serialize, Default)]
struct Row {
    ts: u64,
    cpu_total: Option<f64>,
    cpu_freq: Option<i64>,
    cpu_temp: Option<f64>,
    mem_used: Option<i64>,
    mem_total: Option<i64>,
    gpu_util: Option<f64>,
    gpu_vram_used: Option<i64>,
    gpu_vram_total: Option<i64>,
    gpu_temp: Option<f64>,
    gpu_power: Option<f64>,
    fps: Option<f64>,
    fps_process: Option<String>,
    frame_time: Option<f64>,
    low1: Option<f64>,
    net_down: Option<f64>,
    net_up: Option<f64>,
    cpu_power: Option<f64>,
    low01: Option<f64>,
    stutters: Option<i64>,
    disk_read: Option<f64>,
    disk_write: Option<f64>,
    disk_active: Option<f64>,
}

struct SessionInfo {
    id: i64,
    started_at: u64,
    ended_at: Option<u64>,
}

pub fn export(
    conn: &Connection,
    session_id: i64,
    format: &str,
    out_dir: &Path,
    lang: Lang,
    hw: &[HwSection],
    full_serials: bool,
) -> Result<String, String> {
    let info = conn
        .query_row(
            "SELECT id, started_at, ended_at FROM sessions WHERE id = ?1",
            params![session_id],
            |r| {
                Ok(SessionInfo {
                    id: r.get(0)?,
                    started_at: r.get(1)?,
                    ended_at: r.get(2)?,
                })
            },
        )
        .map_err(|_| {
            tr_fmt(lang, "report.err.noSession", &[("id", &session_id.to_string())])
        })?;

    let rows = load_rows(conn, session_id)?;
    if rows.is_empty() {
        return Err(tr(lang, "report.err.noData").into());
    }

    let ext = match format {
        "html" => "html",
        "csv" => "csv",
        "json" => "json",
        "md" => "md",
        other => return Err(tr_fmt(lang, "report.err.badFormat", &[("fmt", other)])),
    };
    let file_name = format!(
        "session{}_{}.{ext}",
        info.id,
        fmt_ts_file(info.started_at)
    );
    let out_path = out_dir.join(file_name);

    let hw = apply_masking(hw, full_serials);
    let alerts = load_alerts(conn, session_id);
    let content = match format {
        "html" => render_html(&info, &rows, lang, &hw, &alerts),
        // CSV 是行式采样数据，塞机器规格没有意义；且它的列名与 JSON 的字段名
        // 保持英文不变 —— 供 Excel、pandas、脚本消费，翻译会破坏下游解析
        "csv" => render_csv(&rows),
        "json" => render_json(&info, &rows, &hw, &alerts)?,
        "md" => render_md(&info, &rows, lang, &hw, &alerts),
        _ => unreachable!(),
    };
    std::fs::write(&out_path, content).map_err(|e| e.to_string())?;
    Ok(out_path.to_string_lossy().into_owned())
}

/// 会话期间触发的告警。metric 存的是 i18n key，此处按报告语言翻译。
#[derive(Serialize)]
struct AlertRow {
    ts: u64,
    metric: String,
    value: f64,
    threshold: f64,
    unit: String,
}

fn load_alerts(conn: &Connection, session_id: i64) -> Vec<AlertRow> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT ts, metric, value, threshold, unit FROM alerts
         WHERE session_id = ?1 ORDER BY ts",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map(params![session_id], |r| {
        Ok(AlertRow {
            ts: r.get(0)?,
            metric: r.get(1)?,
            value: r.get(2)?,
            threshold: r.get(3)?,
            unit: r.get(4)?,
        })
    });
    rows.map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

fn load_rows(conn: &Connection, session_id: i64) -> Result<Vec<Row>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ts, cpu_total, cpu_freq, cpu_temp, mem_used, mem_total,
                gpu_util, gpu_vram_used, gpu_vram_total, gpu_temp, gpu_power,
                fps, fps_process, frame_time, low1, net_down, net_up,
                cpu_power, low01, stutters, disk_read, disk_write, disk_active
             FROM samples WHERE session_id = ?1 ORDER BY ts",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |r| {
            Ok(Row {
                ts: r.get(0)?,
                cpu_total: r.get(1)?,
                cpu_freq: r.get(2)?,
                cpu_temp: r.get(3)?,
                mem_used: r.get(4)?,
                mem_total: r.get(5)?,
                gpu_util: r.get(6)?,
                gpu_vram_used: r.get(7)?,
                gpu_vram_total: r.get(8)?,
                gpu_temp: r.get(9)?,
                gpu_power: r.get(10)?,
                fps: r.get(11)?,
                fps_process: r.get(12)?,
                frame_time: r.get(13)?,
                low1: r.get(14)?,
                net_down: r.get(15)?,
                net_up: r.get(16)?,
                cpu_power: r.get(17)?,
                low01: r.get(18)?,
                stutters: r.get(19)?,
                disk_read: r.get(20)?,
                disk_write: r.get(21)?,
                disk_active: r.get(22)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ---------- 统计 ----------

struct Stats {
    avg: f64,
    max: f64,
    min: f64,
}

fn stats(vals: impl Iterator<Item = Option<f64>>) -> Option<Stats> {
    let v: Vec<f64> = vals.flatten().collect();
    if v.is_empty() {
        return None;
    }
    Some(Stats {
        avg: v.iter().sum::<f64>() / v.len() as f64,
        max: v.iter().cloned().fold(f64::MIN, f64::max),
        min: v.iter().cloned().fold(f64::MAX, f64::min),
    })
}

fn mem_pct(r: &Row) -> Option<f64> {
    match (r.mem_used, r.mem_total) {
        (Some(u), Some(t)) if t > 0 => Some(u as f64 / t as f64 * 100.0),
        _ => None,
    }
}

fn exceed_pct(rows: &[Row], f: impl Fn(&Row) -> Option<f64>, threshold: f64) -> f64 {
    let (mut n, mut total) = (0usize, 0usize);
    for r in rows {
        if let Some(v) = f(r) {
            total += 1;
            if v >= threshold {
                n += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        n as f64 / total as f64 * 100.0
    }
}

fn fmt_ts(ms: u64) -> String {
    DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn fmt_ts_file(ms: u64) -> String {
    DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.with_timezone(&Local).format("%Y%m%d_%H%M%S").to_string())
        .unwrap_or_default()
}

fn fmt_duration(info: &SessionInfo, rows: &[Row], lang: Lang) -> String {
    let end = info
        .ended_at
        .unwrap_or_else(|| rows.last().map(|r| r.ts).unwrap_or(info.started_at));
    let secs = end.saturating_sub(info.started_at) / 1000;
    tr_fmt(
        lang,
        "report.duration",
        &[
            ("h", &(secs / 3600).to_string()),
            ("m", &(secs % 3600 / 60).to_string()),
            ("s", &(secs % 60).to_string()),
        ],
    )
}

/// 统计行集合：(名称, 单位, 统计值)
fn stat_rows(rows: &[Row], lang: Lang) -> Vec<(&'static str, &'static str, Option<Stats>)> {
    let n = |key| tr(lang, key);
    vec![
        (n("report.metric.cpuUtil"), "%", stats(rows.iter().map(|r| r.cpu_total))),
        (n("report.metric.cpuTemp"), "°C", stats(rows.iter().map(|r| r.cpu_temp))),
        (n("report.metric.cpuPower"), "W", stats(rows.iter().map(|r| r.cpu_power))),
        (n("report.metric.memUtil"), "%", stats(rows.iter().map(mem_pct))),
        (n("report.metric.gpuUtil"), "%", stats(rows.iter().map(|r| r.gpu_util))),
        (n("report.metric.gpuTemp"), "°C", stats(rows.iter().map(|r| r.gpu_temp))),
        (n("report.metric.gpuPower"), "W", stats(rows.iter().map(|r| r.gpu_power))),
        // FPS 三项是通用术语，两种语言下写法一致
        ("FPS", "", stats(rows.iter().map(|r| r.fps))),
        ("FPS 1% Low", "", stats(rows.iter().map(|r| r.low1))),
        ("FPS 0.1% Low", "", stats(rows.iter().map(|r| r.low01))),
        (n("report.metric.frameTime"), "ms", stats(rows.iter().map(|r| r.frame_time))),
        (
            n("report.metric.stutters"),
            "",
            stats(rows.iter().map(|r| r.stutters.map(|v| v as f64))),
        ),
        (
            n("report.metric.diskRead"),
            "MB/s",
            stats(rows.iter().map(|r| r.disk_read.map(|v| v / 1048576.0))),
        ),
        (
            n("report.metric.diskWrite"),
            "MB/s",
            stats(rows.iter().map(|r| r.disk_write.map(|v| v / 1048576.0))),
        ),
        (n("report.metric.diskActive"), "%", stats(rows.iter().map(|r| r.disk_active))),
        (
            n("report.metric.netDown"),
            "MB/s",
            stats(rows.iter().map(|r| r.net_down.map(|v| v / 1048576.0))),
        ),
        (
            n("report.metric.netUp"),
            "MB/s",
            stats(rows.iter().map(|r| r.net_up.map(|v| v / 1048576.0))),
        ),
    ]
}

/// 阈值超限行集合：(条件描述, 超限占比)
fn threshold_rows(rows: &[Row], lang: Lang) -> Vec<(&'static str, f64)> {
    vec![
        (tr(lang, "report.th.cpuUtil"), exceed_pct(rows, |r| r.cpu_total, 90.0)),
        (tr(lang, "report.th.cpuTemp"), exceed_pct(rows, |r| r.cpu_temp, 95.0)),
        (tr(lang, "report.th.memUtil"), exceed_pct(rows, mem_pct, 90.0)),
        (tr(lang, "report.th.gpuUtil"), exceed_pct(rows, |r| r.gpu_util, 90.0)),
        (tr(lang, "report.th.gpuTemp"), exceed_pct(rows, |r| r.gpu_temp, 85.0)),
    ]
}

// ---------- 各格式渲染 ----------

fn render_csv(rows: &[Row]) -> String {
    let mut out = String::from(
        "ts,cpu_total,cpu_freq_mhz,cpu_temp_c,mem_used,mem_total,\
         gpu_util,gpu_vram_used,gpu_vram_total,gpu_temp_c,gpu_power_w,\
         fps,fps_process,frame_time_ms,fps_low1,net_down_bps,net_up_bps,\
         cpu_power_w,fps_low01,stutters,disk_read_bps,disk_write_bps,disk_active_pct\n",
    );
    fn c<T: ToString>(v: &Option<T>) -> String {
        v.as_ref().map(|x| x.to_string()).unwrap_or_default()
    }
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.ts,
            c(&r.cpu_total),
            c(&r.cpu_freq),
            c(&r.cpu_temp),
            c(&r.mem_used),
            c(&r.mem_total),
            c(&r.gpu_util),
            c(&r.gpu_vram_used),
            c(&r.gpu_vram_total),
            c(&r.gpu_temp),
            c(&r.gpu_power),
            c(&r.fps),
            c(&r.fps_process),
            c(&r.frame_time),
            c(&r.low1),
            c(&r.net_down),
            c(&r.net_up),
            c(&r.cpu_power),
            c(&r.low01),
            c(&r.stutters),
            c(&r.disk_read),
            c(&r.disk_write),
            c(&r.disk_active),
        ));
    }
    out
}

fn render_json(
    info: &SessionInfo,
    rows: &[Row],
    hw: &[HwSection],
    alerts: &[AlertRow],
) -> Result<String, String> {
    let v = serde_json::json!({
        "session": {
            "id": info.id,
            "started_at": info.started_at,
            "ended_at": info.ended_at,
            "started_at_local": fmt_ts(info.started_at),
        },
        "hardware": hw,
        "alerts": alerts,
        "samples": rows,
    });
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

fn render_md(
    info: &SessionInfo,
    rows: &[Row],
    lang: Lang,
    hw: &[HwSection],
    alerts: &[AlertRow],
) -> String {
    let mut out = format!(
        "# {}\n\n- {}: {}\n- {}: {}\n- {}: {}\n- {}: {}\n",
        tr_fmt(lang, "report.title", &[("id", &info.id.to_string())]),
        tr(lang, "report.meta.start"),
        fmt_ts(info.started_at),
        tr(lang, "report.meta.end"),
        info.ended_at.map(fmt_ts).unwrap_or_else(|| "—".into()),
        tr(lang, "report.meta.duration"),
        fmt_duration(info, rows, lang),
        tr(lang, "report.meta.samples"),
        rows.len(),
    );
    let procs: Vec<String> = {
        let mut seen = Vec::new();
        for r in rows {
            if let Some(p) = &r.fps_process {
                if !seen.contains(p) {
                    seen.push(p.clone());
                }
            }
        }
        seen
    };
    if !procs.is_empty() {
        out.push_str(&format!(
            "- {}: {}\n",
            tr(lang, "report.fpsProcs"),
            procs.join(", ")
        ));
    }

    out.push_str(&format!(
        "\n## {}\n\n| {} | {} | {} | {} |\n|---|---|---|---|\n",
        tr(lang, "report.statsHeading"),
        tr(lang, "report.col.metric"),
        tr(lang, "report.col.avg"),
        tr(lang, "report.col.max"),
        tr(lang, "report.col.min"),
    ));
    for (name, unit, st) in stat_rows(rows, lang) {
        match st {
            Some(s) => out.push_str(&format!(
                "| {name} | {:.1}{unit} | {:.1}{unit} | {:.1}{unit} |\n",
                s.avg, s.max, s.min
            )),
            None => out.push_str(&format!("| {name} | — | — | — |\n")),
        }
    }

    out.push_str(&format!(
        "\n## {}\n\n| {} | {} |\n|---|---|\n",
        tr(lang, "report.threshHeading"),
        tr(lang, "report.col.condition"),
        tr(lang, "report.col.exceed"),
    ));
    for (name, pct) in threshold_rows(rows, lang) {
        out.push_str(&format!("| {name} | {pct:.1}% |\n"));
    }

    if !alerts.is_empty() {
        out.push_str(&format!(
            "\n## {}\n\n| {} | {} | {} |\n|---|---|---|\n",
            tr(lang, "report.alertsHeading"),
            tr(lang, "report.col.time"),
            tr(lang, "report.col.metric"),
            tr(lang, "report.col.reading"),
        ));
        for a in alerts {
            out.push_str(&format!(
                "| {} | {} | {:.0}{} (≥ {:.0}{}) |\n",
                fmt_ts(a.ts),
                tr(lang, &a.metric),
                a.value,
                a.unit,
                a.threshold,
                a.unit
            ));
        }
    }

    if !hw.is_empty() {
        out.push_str(&format!("\n## {}\n", tr(lang, "report.hwHeading")));
        for sec in hw {
            out.push_str(&format!("\n### {}\n\n", sec.title));
            for b in &sec.blocks {
                if !b.title.is_empty() {
                    out.push_str(&format!("**{}**\n\n", b.title));
                }
                for r in &b.rows {
                    out.push_str(&format!("- {}: {}\n", r.label, r.value));
                }
                out.push('\n');
            }
        }
    }
    out
}

fn render_html(
    info: &SessionInfo,
    rows: &[Row],
    lang: Lang,
    hw: &[HwSection],
    alerts: &[AlertRow],
) -> String {
    let data = serde_json::json!({
        "ts": rows.iter().map(|r| r.ts / 1000).collect::<Vec<_>>(),
        "cpu": rows.iter().map(|r| r.cpu_total).collect::<Vec<_>>(),
        "cpuTemp": rows.iter().map(|r| r.cpu_temp).collect::<Vec<_>>(),
        "memPct": rows.iter().map(mem_pct).collect::<Vec<_>>(),
        "gpu": rows.iter().map(|r| r.gpu_util).collect::<Vec<_>>(),
        "gpuTemp": rows.iter().map(|r| r.gpu_temp).collect::<Vec<_>>(),
        "gpuPower": rows.iter().map(|r| r.gpu_power).collect::<Vec<_>>(),
        "vramPct": rows.iter().map(|r| match (r.gpu_vram_used, r.gpu_vram_total) {
            (Some(u), Some(t)) if t > 0 => Some(u as f64 / t as f64 * 100.0),
            _ => None,
        }).collect::<Vec<_>>(),
        "fps": rows.iter().map(|r| r.fps).collect::<Vec<_>>(),
        "netDown": rows.iter().map(|r| r.net_down.map(|v| v / 1048576.0)).collect::<Vec<_>>(),
        "netUp": rows.iter().map(|r| r.net_up.map(|v| v / 1048576.0)).collect::<Vec<_>>(),
        "cpuPower": rows.iter().map(|r| r.cpu_power).collect::<Vec<_>>(),
        "diskRead": rows.iter().map(|r| r.disk_read.map(|v| v / 1048576.0)).collect::<Vec<_>>(),
        "diskWrite": rows.iter().map(|r| r.disk_write.map(|v| v / 1048576.0)).collect::<Vec<_>>(),
    });

    let mut stats_html = String::new();
    for (name, unit, st) in stat_rows(rows, lang) {
        match st {
            Some(s) => stats_html.push_str(&format!(
                "<tr><td>{name}</td><td>{:.1}{unit}</td><td>{:.1}{unit}</td><td>{:.1}{unit}</td></tr>",
                s.avg, s.max, s.min
            )),
            None => stats_html
                .push_str(&format!("<tr><td>{name}</td><td>—</td><td>—</td><td>—</td></tr>")),
        }
    }

    let mut thresh_html = String::new();
    for (name, pct) in threshold_rows(rows, lang) {
        let cls = if pct > 0.0 { " class=\"bad\"" } else { "" };
        thresh_html.push_str(&format!("<tr><td>{name}</td><td{cls}>{pct:.1}%</td></tr>"));
    }

    let meta_html = format!(
        "<span>{} <b>{}</b></span><span>{} <b>{}</b></span>\
         <span>{} <b>{}</b></span><span>{} <b>{}</b></span>",
        tr(lang, "report.meta.start"),
        fmt_ts(info.started_at),
        tr(lang, "report.meta.end"),
        info.ended_at.map(fmt_ts).unwrap_or_else(|| "—".into()),
        tr(lang, "report.meta.duration"),
        fmt_duration(info, rows, lang),
        tr(lang, "report.meta.samples"),
        rows.len(),
    );

    let mut alert_html = String::new();
    if !alerts.is_empty() {
        alert_html.push_str(&format!(
            "<h2>{}</h2><table><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
            esc(tr(lang, "report.alertsHeading")),
            esc(tr(lang, "report.col.time")),
            esc(tr(lang, "report.col.metric")),
            esc(tr(lang, "report.col.reading")),
        ));
        for a in alerts {
            alert_html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td class=\"bad\">{:.0}{} (≥ {:.0}{})</td></tr>",
                esc(&fmt_ts(a.ts)),
                esc(tr(lang, &a.metric)),
                a.value,
                esc(&a.unit),
                a.threshold,
                esc(&a.unit)
            ));
        }
        alert_html.push_str("</tbody></table>");
    }

    // 硬件信息里含设备自报的型号串，一律转义后再拼进 HTML
    let mut hw_html = String::new();
    if !hw.is_empty() {
        hw_html.push_str(&format!(
            "<h2>{}</h2>",
            esc(tr(lang, "report.hwHeading"))
        ));
        for sec in hw {
            hw_html.push_str(&format!(
                "<div class=\"hw-sec\"><h3>{}</h3>",
                esc(&sec.title)
            ));
            for b in &sec.blocks {
                if !b.title.is_empty() {
                    hw_html.push_str(&format!("<h4>{}</h4>", esc(&b.title)));
                }
                hw_html.push_str("<table><tbody>");
                for r in &b.rows {
                    hw_html.push_str(&format!(
                        "<tr><td>{}</td><td>{}</td></tr>",
                        esc(&r.label),
                        esc(&r.value)
                    ));
                }
                hw_html.push_str("</tbody></table>");
            }
            hw_html.push_str("</div>");
        }
    }

    let mut html = HTML_TEMPLATE
        .replace("__ALERTS__", &alert_html)
        .replace("__HARDWARE__", &hw_html)
        .replace(
            "__TITLE__",
            &tr_fmt(lang, "report.title", &[("id", &info.id.to_string())]),
        )
        .replace("__META__", &meta_html)
        .replace("__STATS__", &stats_html)
        .replace("__THRESH__", &thresh_html)
        .replace("__UPLOT_CSS__", UPLOT_CSS)
        .replace("__UPLOT_JS__", UPLOT_JS)
        .replace("__DATA__", &escape_json_for_script(data.to_string()));
    for (token, key) in TEMPLATE_STRINGS {
        html = html.replace(token, tr(lang, key));
    }
    html
}

/// 模板中的固定文案占位符 → i18n key。部分值会落进 <script> 里的 JS 字符串
/// 字面量，i18n 的 report_strings_are_safe_to_embed 测试保证它们不含引号或尖括号。
const TEMPLATE_STRINGS: &[(&str, &str)] = &[
    ("__T_LANGCODE__", "report.langCode"),
    ("__T_PRINT__", "report.print"),
    ("__T_STATS_H__", "report.statsHeading"),
    ("__T_THRESH_H__", "report.threshHeading"),
    ("__T_CHARTS_H__", "report.chartsHeading"),
    ("__T_COL_METRIC__", "report.col.metric"),
    ("__T_COL_AVG__", "report.col.avg"),
    ("__T_COL_MAX__", "report.col.max"),
    ("__T_COL_MIN__", "report.col.min"),
    ("__T_COL_COND__", "report.col.condition"),
    ("__T_COL_EXCEED__", "report.col.exceed"),
    ("__T_CHART_UTIL__", "report.chart.util"),
    ("__T_CHART_TEMP__", "report.chart.temp"),
    ("__T_CHART_NET__", "report.chart.net"),
    ("__T_CHART_DISK__", "report.chart.disk"),
    ("__T_CHART_POWER__", "report.chart.power"),
    ("__T_L_MEM__", "report.label.mem"),
    ("__T_L_VRAM__", "report.label.vram"),
    ("__T_L_DOWN__", "report.label.down"),
    ("__T_L_UP__", "report.label.up"),
    ("__T_L_READ__", "report.label.read"),
    ("__T_L_WRITE__", "report.label.write"),
];

/// 硬件信息中的型号、适配器名等来自驱动与固件，是外部字符串，
/// 拼进 HTML 前必须转义
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 嵌入 <script> 块前转义 JSON 中的 `<`，防止字符串值（如进程名）
/// 包含 </script> 逃逸出脚本块。JSON 语法本身不含 `<`，
/// 替换只作用于字符串字面量内部，解析语义不变
fn escape_json_for_script(json: String) -> String {
    json.replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
    use crate::i18n::Lang;
    use crate::recorder::open_db;
    use crate::recorder::tests::make_test_db;

    #[test]
    fn export_all_formats() {
        let dir = std::env::temp_dir().join("sysscope-test-report");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();

        for (lang, fmt, marker) in [
            (Lang::ZhCn, "html", "SysScope 报告"),
            (Lang::En, "html", "SysScope report"),
            // CSV/JSON 不随语言变化：列名与字段名是给下游程序消费的
            (Lang::ZhCn, "csv", "cpu_total"),
            (Lang::En, "csv", "cpu_total"),
            (Lang::ZhCn, "json", "\"samples\""),
            (Lang::En, "json", "\"samples\""),
            (Lang::ZhCn, "md", "## 统计摘要"),
            (Lang::En, "md", "## Summary"),
        ] {
            let path = super::export(&conn, sid, fmt, &dir, lang, &[], false).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                content.contains(marker),
                "{fmt}/{lang:?} report missing marker {marker}"
            );
        }

        // 英文报告不得残留未替换的占位符或中文
        let en_html =
            std::fs::read_to_string(super::export(&conn, sid, "html", &dir, Lang::En, &[], false).unwrap())
                .unwrap();
        assert!(!en_html.contains("__T_"), "unreplaced template placeholder");
        for zh in ["统计摘要", "阈值超限", "历史曲线", "打印", "内存", "显存"] {
            assert!(!en_html.contains(zh), "English report still contains {zh}");
        }

        // HTML 报告应自包含（内嵌 uPlot 与数据）
        let html =
            std::fs::read_to_string(super::export(&conn, sid, "html", &dir, Lang::ZhCn, &[], false).unwrap())
                .unwrap();
        assert!(html.contains("uPlot"), "uPlot not embedded");
        assert!(html.contains("\"cpu\""), "data not embedded");
        assert!(!html.contains("http://") || html.contains("http://www.w3.org"), "external refs");

        // 未知格式与空会话报错，且错误文案跟随语言
        let err = super::export(&conn, sid, "pdf", &dir, Lang::En, &[], false).unwrap_err();
        assert_eq!(err, "Unknown format: pdf");
        let err = super::export(&conn, 9999, "html", &dir, Lang::En, &[], false).unwrap_err();
        assert_eq!(err, "Session 9999 does not exist");
        assert!(super::export(&conn, 9999, "html", &dir, Lang::ZhCn, &[], false).is_err());
    }

    fn sample_hw() -> Vec<super::HwSection> {
        vec![super::HwSection {
            title: "Motherboard".into(),
            blocks: vec![super::HwBlock {
                title: String::new(),
                rows: vec![
                    super::HwRow {
                        label: "Board model".into(),
                        value: "ROG STRIX B760-G".into(),
                        sensitive: false,
                    },
                    super::HwRow {
                        label: "Board serial".into(),
                        value: "240436541701519".into(),
                        sensitive: true,
                    },
                ],
            }],
        }]
    }

    #[test]
    fn mask_keeps_only_last_four() {
        assert_eq!(super::mask("240436541701519"), "****1519");
        assert_eq!(super::mask("10:7C:61:B5:C5:2B"), "****5:2B");
        // 过短的值整体打码，不能反而把全部内容留下
        assert_eq!(super::mask("AB"), "**");
        assert_eq!(super::mask(""), "*");
        // 多字节字符按字符切分，不能在 UTF-8 中间截断导致 panic
        assert_eq!(super::mask("序列号很长的值"), "****很长的值");
    }

    /// 未开启完整显示时，序列号不得以任何形式进入导出文件
    #[test]
    fn serials_are_masked_in_every_format() {
        let dir = std::env::temp_dir().join("sysscope-test-mask");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();
        let hw = sample_hw();

        for fmt in ["html", "md", "json"] {
            let path =
                super::export(&conn, sid, fmt, &dir, Lang::En, &hw, false).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                !content.contains("240436541701519"),
                "{fmt}: raw serial leaked into the report"
            );
            assert!(content.contains("****1519"), "{fmt}: masked serial missing");
            // 非敏感字段照常完整输出
            assert!(content.contains("ROG STRIX B760-G"), "{fmt}: model missing");
        }

        // 显式开启后才输出完整序列号
        let path = super::export(&conn, sid, "md", &dir, Lang::En, &hw, true).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("240436541701519"));
    }

    /// 会话期间的告警必须出现在报告里 —— 否则录制时弹过的通知事后无处可查
    #[test]
    fn alerts_appear_in_reports() {
        let dir = std::env::temp_dir().join("sysscope-test-alerts");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();
        conn.execute(
            "INSERT INTO alerts (session_id, ts, metric, value, threshold, unit)
             VALUES (?1, 62000, 'alert.metric.cpuTemp', 97.5, 95.0, '°C')",
            rusqlite::params![sid],
        )
        .unwrap();

        for (lang, fmt, heading) in [
            (Lang::En, "html", "Alerts"),
            (Lang::En, "md", "## Alerts"),
            (Lang::ZhCn, "md", "## 告警记录"),
        ] {
            let path = super::export(&conn, sid, fmt, &dir, lang, &[], false).unwrap();
            let c = std::fs::read_to_string(&path).unwrap();
            assert!(c.contains(heading), "{fmt}/{lang:?} missing alert section");
            // metric 存的是 i18n key，报告里必须已被翻译
            assert!(
                !c.contains("alert.metric.cpuTemp"),
                "{fmt}/{lang:?} leaked a raw i18n key"
            );
        }
        // 英文报告里应显示翻译后的指标名与读数
        let p = super::export(&conn, sid, "md", &dir, Lang::En, &[], false).unwrap();
        let md = std::fs::read_to_string(p).unwrap();
        assert!(md.contains("CPU temperature"), "{md}");
        assert!(md.contains("98°C"), "{md}");
    }

    /// 没有告警时不应留下空标题
    #[test]
    fn no_alert_section_when_none_fired() {
        let dir = std::env::temp_dir().join("sysscope-test-noalerts");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();
        let p = super::export(&conn, sid, "md", &dir, Lang::En, &[], false).unwrap();
        assert!(!std::fs::read_to_string(p).unwrap().contains("## Alerts"));
    }

    /// CSV 是行式采样数据，不该混入机器规格
    #[test]
    fn csv_carries_no_hardware() {
        let dir = std::env::temp_dir().join("sysscope-test-csvhw");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();
        let path =
            super::export(&conn, sid, "csv", &dir, Lang::En, &sample_hw(), true).unwrap();
        let csv = std::fs::read_to_string(&path).unwrap();
        assert!(!csv.contains("ROG STRIX"));
        assert!(!csv.contains("240436541701519"));
    }

    /// 型号串来自驱动与固件，必须转义后才进 HTML
    #[test]
    fn hardware_values_are_html_escaped() {
        let dir = std::env::temp_dir().join("sysscope-test-hwesc");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();
        let hw = vec![super::HwSection {
            title: "X".into(),
            blocks: vec![super::HwBlock {
                title: String::new(),
                rows: vec![super::HwRow {
                    label: "Model".into(),
                    value: "<script>alert(1)</script>".into(),
                    sensitive: false,
                }],
            }],
        }];
        let path = super::export(&conn, sid, "html", &dir, Lang::En, &hw, false).unwrap();
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(!html.contains("<script>alert(1)"), "unescaped device string");
        assert!(html.contains("&lt;script&gt;alert(1)"));
    }

    /// 恶意进程名不得逃逸出 HTML 报告的 <script> 数据块
    #[test]
    fn html_report_escapes_hostile_process_names() {
        let dir = std::env::temp_dir().join("sysscope-test-xss");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();
        conn.execute(
            "INSERT INTO samples (session_id, ts, cpu_total, fps, fps_process)
             VALUES (?1, 62000, 10.0, 60.0, ?2)",
            rusqlite::params![sid, "</script><script>alert(1)</script>"],
        )
        .unwrap();

        let path = super::export(&conn, sid, "html", &dir, Lang::ZhCn, &[], false).unwrap();
        let html = std::fs::read_to_string(path).unwrap();
        assert!(
            !html.contains("</script><script>alert"),
            "hostile process name escaped the data block"
        );
    }

    /// 转义函数本身的行为：任何 `<` 不得原样出现
    #[test]
    fn script_escape_neutralizes_lt() {
        let json = r#"{"p":"</script><script>alert(1)</script>"}"#.to_string();
        let escaped = super::escape_json_for_script(json);
        assert!(!escaped.contains('<'));
        assert!(escaped.contains("\\u003c/script"));
        // 转义后仍是合法 JSON 且值不变
        let v: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(v["p"], "</script><script>alert(1)</script>");
    }
}
