use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

const UPLOT_JS: &str = include_str!("../templates/uPlot.iife.min.js");
const UPLOT_CSS: &str = include_str!("../templates/uPlot.min.css");
const HTML_TEMPLATE: &str = include_str!("../templates/report.html");

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
        .map_err(|_| format!("会话 {session_id} 不存在"))?;

    let rows = load_rows(conn, session_id)?;
    if rows.is_empty() {
        return Err("该会话没有采样数据".into());
    }

    let ext = match format {
        "html" => "html",
        "csv" => "csv",
        "json" => "json",
        "md" => "md",
        other => return Err(format!("未知格式: {other}")),
    };
    let file_name = format!(
        "session{}_{}.{ext}",
        info.id,
        fmt_ts_file(info.started_at)
    );
    let out_path = out_dir.join(file_name);

    let content = match format {
        "html" => render_html(&info, &rows),
        "csv" => render_csv(&rows),
        "json" => render_json(&info, &rows)?,
        "md" => render_md(&info, &rows),
        _ => unreachable!(),
    };
    std::fs::write(&out_path, content).map_err(|e| e.to_string())?;
    Ok(out_path.to_string_lossy().into_owned())
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

fn fmt_duration(info: &SessionInfo, rows: &[Row]) -> String {
    let end = info
        .ended_at
        .unwrap_or_else(|| rows.last().map(|r| r.ts).unwrap_or(info.started_at));
    let secs = end.saturating_sub(info.started_at) / 1000;
    format!("{}时{}分{}秒", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// 统计行集合：(名称, 单位, 统计值)
fn stat_rows(rows: &[Row]) -> Vec<(&'static str, &'static str, Option<Stats>)> {
    vec![
        ("CPU 占用", "%", stats(rows.iter().map(|r| r.cpu_total))),
        ("CPU 温度", "°C", stats(rows.iter().map(|r| r.cpu_temp))),
        ("CPU 功耗", "W", stats(rows.iter().map(|r| r.cpu_power))),
        ("内存占用", "%", stats(rows.iter().map(mem_pct))),
        ("GPU 占用", "%", stats(rows.iter().map(|r| r.gpu_util))),
        ("GPU 温度", "°C", stats(rows.iter().map(|r| r.gpu_temp))),
        ("GPU 功耗", "W", stats(rows.iter().map(|r| r.gpu_power))),
        ("FPS", "", stats(rows.iter().map(|r| r.fps))),
        ("FPS 1% Low", "", stats(rows.iter().map(|r| r.low1))),
        ("FPS 0.1% Low", "", stats(rows.iter().map(|r| r.low01))),
        ("帧时间", "ms", stats(rows.iter().map(|r| r.frame_time))),
        (
            "卡顿次数/5s",
            "",
            stats(rows.iter().map(|r| r.stutters.map(|v| v as f64))),
        ),
        (
            "磁盘读取",
            "MB/s",
            stats(rows.iter().map(|r| r.disk_read.map(|v| v / 1048576.0))),
        ),
        (
            "磁盘写入",
            "MB/s",
            stats(rows.iter().map(|r| r.disk_write.map(|v| v / 1048576.0))),
        ),
        ("磁盘活动", "%", stats(rows.iter().map(|r| r.disk_active))),
        (
            "下载速率",
            "MB/s",
            stats(rows.iter().map(|r| r.net_down.map(|v| v / 1048576.0))),
        ),
        (
            "上传速率",
            "MB/s",
            stats(rows.iter().map(|r| r.net_up.map(|v| v / 1048576.0))),
        ),
    ]
}

/// 阈值超限行集合：(条件描述, 超限占比)
fn threshold_rows(rows: &[Row]) -> Vec<(&'static str, f64)> {
    vec![
        ("CPU 占用 ≥ 90%", exceed_pct(rows, |r| r.cpu_total, 90.0)),
        ("CPU 温度 ≥ 95°C", exceed_pct(rows, |r| r.cpu_temp, 95.0)),
        ("内存占用 ≥ 90%", exceed_pct(rows, mem_pct, 90.0)),
        ("GPU 占用 ≥ 90%", exceed_pct(rows, |r| r.gpu_util, 90.0)),
        ("GPU 温度 ≥ 85°C", exceed_pct(rows, |r| r.gpu_temp, 85.0)),
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

fn render_json(info: &SessionInfo, rows: &[Row]) -> Result<String, String> {
    let v = serde_json::json!({
        "session": {
            "id": info.id,
            "started_at": info.started_at,
            "ended_at": info.ended_at,
            "started_at_local": fmt_ts(info.started_at),
        },
        "samples": rows,
    });
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

fn render_md(info: &SessionInfo, rows: &[Row]) -> String {
    let mut out = format!(
        "# SysScope 监控报告 — 会话 #{}\n\n\
         - 开始时间：{}\n- 结束时间：{}\n- 时长：{}\n- 采样点数：{}\n",
        info.id,
        fmt_ts(info.started_at),
        info.ended_at.map(fmt_ts).unwrap_or_else(|| "—".into()),
        fmt_duration(info, rows),
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
        out.push_str(&format!("- FPS 监控进程：{}\n", procs.join("、")));
    }

    out.push_str("\n## 统计摘要\n\n| 指标 | 平均 | 峰值 | 最低 |\n|---|---|---|---|\n");
    for (name, unit, st) in stat_rows(rows) {
        match st {
            Some(s) => out.push_str(&format!(
                "| {name} | {:.1}{unit} | {:.1}{unit} | {:.1}{unit} |\n",
                s.avg, s.max, s.min
            )),
            None => out.push_str(&format!("| {name} | — | — | — |\n")),
        }
    }

    out.push_str("\n## 阈值超限\n\n| 条件 | 超限采样占比 |\n|---|---|\n");
    for (name, pct) in threshold_rows(rows) {
        out.push_str(&format!("| {name} | {pct:.1}% |\n"));
    }
    out
}

fn render_html(info: &SessionInfo, rows: &[Row]) -> String {
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
    for (name, unit, st) in stat_rows(rows) {
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
    for (name, pct) in threshold_rows(rows) {
        let cls = if pct > 0.0 { " class=\"bad\"" } else { "" };
        thresh_html.push_str(&format!("<tr><td>{name}</td><td{cls}>{pct:.1}%</td></tr>"));
    }

    let meta_html = format!(
        "<span>开始 <b>{}</b></span><span>结束 <b>{}</b></span>\
         <span>时长 <b>{}</b></span><span>采样 <b>{}</b></span>",
        fmt_ts(info.started_at),
        info.ended_at.map(fmt_ts).unwrap_or_else(|| "—".into()),
        fmt_duration(info, rows),
        rows.len(),
    );

    HTML_TEMPLATE
        .replace("__TITLE__", &format!("SysScope 报告 — 会话 #{}", info.id))
        .replace("__META__", &meta_html)
        .replace("__STATS__", &stats_html)
        .replace("__THRESH__", &thresh_html)
        .replace("__UPLOT_CSS__", UPLOT_CSS)
        .replace("__UPLOT_JS__", UPLOT_JS)
        .replace("__DATA__", &data.to_string())
}

#[cfg(test)]
mod tests {
    use crate::recorder::tests::make_test_db;
    use crate::recorder::open_db;

    #[test]
    fn export_all_formats() {
        let dir = std::env::temp_dir().join("sysscope-test-report");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);
        let conn = open_db(&db_path).unwrap();

        for (fmt, marker) in [
            ("html", "SysScope 报告"),
            ("csv", "cpu_total"),
            ("json", "\"samples\""),
            ("md", "## 统计摘要"),
        ] {
            let path = super::export(&conn, sid, fmt, &dir).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                content.contains(marker),
                "{fmt} report missing marker {marker}"
            );
        }

        // HTML 报告应自包含（内嵌 uPlot 与数据）
        let html = std::fs::read_to_string(super::export(&conn, sid, "html", &dir).unwrap()).unwrap();
        assert!(html.contains("uPlot"), "uPlot not embedded");
        assert!(html.contains("\"cpu\""), "data not embedded");
        assert!(!html.contains("http://") || html.contains("http://www.w3.org"), "external refs");

        // 未知格式与空会话报错
        assert!(super::export(&conn, sid, "pdf", &dir).is_err());
        assert!(super::export(&conn, 9999, "html", &dir).is_err());
    }
}
