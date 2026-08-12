use crate::sampler::Snapshot;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;

/// 保留的最大会话数，超出部分启动时清理
const MAX_SESSIONS: i64 = 30;

// ---------- 共享控制状态（命令线程 <-> 采样线程） ----------

#[derive(Serialize, Clone, Default)]
pub struct RecStatus {
    pub active: bool,
    pub session_id: Option<i64>,
    /// Unix 毫秒
    pub started_at: Option<u64>,
    pub samples: u64,
}

#[derive(Default)]
pub struct RecorderCtl {
    pub requested: AtomicBool,
    pub status: Mutex<RecStatus>,
}

/// 数据库路径（tauri 托管状态）
pub struct DbPath(pub PathBuf);

// ---------- 表结构 ----------

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at INTEGER NOT NULL,
            ended_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS samples (
            session_id INTEGER NOT NULL,
            ts INTEGER NOT NULL,
            cpu_total REAL, cpu_freq INTEGER, cpu_temp REAL,
            mem_used INTEGER, mem_total INTEGER,
            gpu_util REAL, gpu_vram_used INTEGER, gpu_vram_total INTEGER,
            gpu_temp REAL, gpu_power REAL,
            fps REAL, fps_process TEXT, frame_time REAL, low1 REAL,
            net_down REAL, net_up REAL
        );
        CREATE INDEX IF NOT EXISTS idx_samples_session ON samples(session_id, ts);",
    )?;
    // 幂等迁移：v2 新增列（已存在时 ALTER 失败，忽略即可）
    for stmt in [
        "ALTER TABLE samples ADD COLUMN cpu_power REAL",
        "ALTER TABLE samples ADD COLUMN low01 REAL",
        "ALTER TABLE samples ADD COLUMN stutters INTEGER",
        "ALTER TABLE samples ADD COLUMN disk_read REAL",
        "ALTER TABLE samples ADD COLUMN disk_write REAL",
        "ALTER TABLE samples ADD COLUMN disk_active REAL",
    ] {
        let _ = conn.execute(stmt, []);
    }
    Ok(())
}

pub fn open_db(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    // WAL + busy_timeout：录制写入与报告导出的连接并发时避免 SQLITE_BUSY
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.busy_timeout(std::time::Duration::from_secs(3));
    init_schema(&conn)?;
    Ok(conn)
}

/// 启动时清理：闭合孤儿会话（上次进程退出时未落 ended_at 的，
/// 以其最后一个采样时刻收尾，使其可正常导出），并只保留最近 MAX_SESSIONS 个
pub fn prune_old_sessions(path: &Path) {
    let Ok(conn) = open_db(path) else { return };
    let _ = conn.execute(
        "UPDATE sessions SET ended_at =
            (SELECT MAX(ts) FROM samples WHERE session_id = sessions.id)
         WHERE ended_at IS NULL
           AND EXISTS (SELECT 1 FROM samples WHERE session_id = sessions.id)",
        [],
    );
    // 无任何采样的空孤儿会话直接删除
    let _ = conn.execute(
        "DELETE FROM sessions WHERE ended_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM samples WHERE session_id = sessions.id)",
        [],
    );
    let _ = conn.execute(
        "DELETE FROM samples WHERE session_id NOT IN
            (SELECT id FROM sessions ORDER BY started_at DESC LIMIT ?1)",
        params![MAX_SESSIONS],
    );
    let _ = conn.execute(
        "DELETE FROM sessions WHERE id NOT IN
            (SELECT id FROM sessions ORDER BY started_at DESC LIMIT ?1)",
        params![MAX_SESSIONS],
    );
}

// ---------- 采样线程侧：录制器 ----------

pub struct Recorder {
    ctl: Arc<RecorderCtl>,
    db_path: PathBuf,
    conn: Option<Connection>,
    session_id: Option<i64>,
    samples: u64,
}

impl Recorder {
    pub fn new(ctl: Arc<RecorderCtl>, db_path: PathBuf) -> Self {
        Recorder {
            ctl,
            db_path,
            conn: None,
            session_id: None,
            samples: 0,
        }
    }

    /// 当前是否有进行中的会话（供采样循环判断请求状态是否有待处理的变化）
    pub fn is_active(&self) -> bool {
        self.session_id.is_some()
    }

    /// 每个采样周期调用一次；根据请求标志开启/写入/结束会话
    pub fn tick(&mut self, snapshot: &Snapshot) {
        let want = self.ctl.requested.load(Ordering::Relaxed);
        match (want, self.session_id) {
            (true, None) => self.start(snapshot),
            (true, Some(id)) => self.record(id, snapshot),
            (false, Some(id)) => self.finish(id, snapshot.ts),
            (false, None) => {}
        }
    }

    fn start(&mut self, snapshot: &Snapshot) {
        if self.conn.is_none() {
            match open_db(&self.db_path) {
                Ok(c) => self.conn = Some(c),
                Err(e) => {
                    eprintln!("[sysscope] recorder db open failed: {e}");
                    self.ctl.requested.store(false, Ordering::Relaxed);
                    return;
                }
            }
        }
        let conn = self.conn.as_ref().unwrap();
        let inserted = conn.execute(
            "INSERT INTO sessions (started_at) VALUES (?1)",
            params![snapshot.ts],
        );
        if let Err(e) = &inserted {
            eprintln!("[sysscope] recorder: create session failed: {e}");
        }
        if inserted.is_ok() {
            let id = conn.last_insert_rowid();
            self.session_id = Some(id);
            self.samples = 0;
            *self.ctl.status.lock().unwrap() = RecStatus {
                active: true,
                session_id: Some(id),
                started_at: Some(snapshot.ts),
                samples: 0,
            };
            self.record(id, snapshot);
        }
    }

    fn record(&mut self, session_id: i64, s: &Snapshot) {
        let Some(conn) = self.conn.as_ref() else { return };
        let gpu = s.gpus.first();
        let fps_ok = s.fps.status == "ok" && s.fps.has_data;
        let disk_read: f64 = s.storage.disks.iter().map(|d| d.read_bps).sum();
        let disk_write: f64 = s.storage.disks.iter().map(|d| d.write_bps).sum();
        let disk_active = s
            .storage
            .disks
            .iter()
            .map(|d| d.active_pct)
            .fold(0.0f32, f32::max);
        let r = conn.execute(
            "INSERT INTO samples (session_id, ts,
                cpu_total, cpu_freq, cpu_temp,
                mem_used, mem_total,
                gpu_util, gpu_vram_used, gpu_vram_total, gpu_temp, gpu_power,
                fps, fps_process, frame_time, low1,
                net_down, net_up,
                cpu_power, low01, stutters, disk_read, disk_write, disk_active)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                     ?19,?20,?21,?22,?23,?24)",
            params![
                session_id,
                s.ts,
                s.cpu.total,
                s.cpu.freq_mhz,
                s.cpu.temp_c,
                s.mem.used,
                s.mem.total,
                gpu.map(|g| g.util_pct),
                gpu.map(|g| g.vram_used),
                gpu.map(|g| g.vram_total),
                gpu.and_then(|g| g.temp_c),
                gpu.and_then(|g| g.power_w),
                fps_ok.then_some(s.fps.metrics.fps),
                fps_ok.then(|| s.fps.process.clone()),
                fps_ok.then_some(s.fps.metrics.frame_time_ms),
                fps_ok.then_some(s.fps.metrics.low_1pct_fps),
                s.net.down_bps,
                s.net.up_bps,
                s.cpu.power_w,
                fps_ok.then_some(s.fps.metrics.low_01pct_fps),
                fps_ok.then_some(s.fps.metrics.stutters),
                disk_read,
                disk_write,
                disk_active,
            ],
        );
        match r {
            Ok(_) => {
                self.samples += 1;
                self.ctl.status.lock().unwrap().samples = self.samples;
            }
            Err(e) => eprintln!("[sysscope] recorder: insert sample failed: {e}"),
        }
    }

    fn finish(&mut self, session_id: i64, ts: u64) {
        if let Some(conn) = self.conn.as_ref() {
            let _ = conn.execute(
                "UPDATE sessions SET ended_at = ?1 WHERE id = ?2",
                params![ts, session_id],
            );
        }
        self.session_id = None;
        *self.ctl.status.lock().unwrap() = RecStatus::default();
    }
}

// ---------- 命令 ----------

#[tauri::command]
pub fn start_recording(ctl: State<Arc<RecorderCtl>>) {
    ctl.requested.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub fn stop_recording(ctl: State<Arc<RecorderCtl>>) {
    ctl.requested.store(false, Ordering::Relaxed);
}

#[tauri::command]
pub fn recording_status(ctl: State<Arc<RecorderCtl>>) -> RecStatus {
    ctl.status.lock().unwrap().clone()
}

#[derive(Serialize)]
pub struct SessionMeta {
    pub id: i64,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub samples: u64,
}

#[tauri::command]
pub fn list_sessions(db: State<DbPath>) -> Result<Vec<SessionMeta>, String> {
    let conn = open_db(&db.0).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.started_at, s.ended_at,
                (SELECT COUNT(*) FROM samples WHERE session_id = s.id)
             FROM sessions s ORDER BY s.started_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SessionMeta {
                id: r.get(0)?,
                started_at: r.get(1)?,
                ended_at: r.get(2)?,
                samples: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
pub fn delete_session(db: State<DbPath>, session_id: i64) -> Result<(), String> {
    let conn = open_db(&db.0).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM samples WHERE session_id = ?1", params![session_id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn export_report(
    db: State<DbPath>,
    session_id: i64,
    format: String,
) -> Result<String, String> {
    let conn = open_db(&db.0).map_err(|e| e.to_string())?;
    let reports_dir = db.0.parent().unwrap_or(Path::new(".")).join("reports");
    std::fs::create_dir_all(&reports_dir).map_err(|e| e.to_string())?;
    crate::report::export(&conn, session_id, &format, &reports_dir)
}

/// 在资源管理器中定位导出的文件。
/// 注意：/select 与路径必须是同一个参数（逗号相连），拆成两个参数会被
/// explorer 忽略而落到默认目录
#[tauri::command]
pub fn open_in_folder(path: String) {
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
}

/// 直接打开报告目录（不存在则先创建），返回其绝对路径供前端展示
#[tauri::command]
pub fn open_reports_dir(db: State<DbPath>) -> Result<String, String> {
    let dir = db.0.parent().unwrap_or(Path::new(".")).join("reports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    Ok(dir.to_string_lossy().into_owned())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// 构造一段合成会话数据（60 个采样点），供本模块与 report 模块测试复用
    pub fn make_test_db(dir: &Path) -> (PathBuf, i64) {
        let db_path = dir.join("test.db");
        let conn = open_db(&db_path).unwrap();
        conn.execute("INSERT INTO sessions (started_at, ended_at) VALUES (1000, 61000)", [])
            .unwrap();
        let sid = conn.last_insert_rowid();
        for i in 0..60i64 {
            conn.execute(
                "INSERT INTO samples (session_id, ts,
                    cpu_total, cpu_freq, cpu_temp, mem_used, mem_total,
                    gpu_util, gpu_vram_used, gpu_vram_total, gpu_temp, gpu_power,
                    fps, fps_process, frame_time, low1, net_down, net_up)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    sid,
                    1000 + i * 1000,
                    30.0 + (i % 10) as f64,          // cpu_total
                    4500,
                    55.0 + (i % 5) as f64,           // cpu_temp
                    16_000_000_000i64 + i * 10_000_000,
                    34_000_000_000i64,
                    20.0 + (i % 40) as f64,          // gpu_util
                    3_000_000_000i64,
                    17_000_000_000i64,
                    40.0 + (i % 8) as f64,
                    60.0 + i as f64,
                    if i % 7 == 0 { None } else { Some(120.0 + (i % 30) as f64) },
                    if i % 7 == 0 { None::<String> } else { Some("game.exe".into()) },
                    Some(8.0),
                    Some(90.0),
                    1_500_000.0,
                    200_000.0,
                ],
            )
            .unwrap();
        }
        (db_path, sid)
    }

    /// 端到端：真实采样快照驱动 开始→写入→停止 全流程
    #[test]
    #[ignore = "hw: 依赖完整采集环境（WMI/ETW），本地 --include-ignored 运行"]
    fn recording_lifecycle_with_real_snapshots() {
        use crate::sampler::{take_snapshot, SamplerCtx};
        let dir = std::env::temp_dir().join("sysscope-test-lifecycle");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("rec.db");

        let ctl = Arc::new(RecorderCtl::default());
        let mut rec = Recorder::new(ctl.clone(), db_path.clone());
        let mut ctx = SamplerCtx::init();

        // 未请求录制：tick 应无副作用
        rec.tick(&take_snapshot(&mut ctx, 0.5));
        assert!(!ctl.status.lock().unwrap().active);

        // 开始录制
        ctl.requested.store(true, Ordering::Relaxed);
        rec.tick(&take_snapshot(&mut ctx, 0.5));
        rec.tick(&take_snapshot(&mut ctx, 0.5));
        let st = ctl.status.lock().unwrap().clone();
        println!("recording: active={} samples={}", st.active, st.samples);
        assert!(st.active, "recording should be active");
        assert!(st.samples >= 2, "samples should accumulate");
        let sid = st.session_id.unwrap();

        // 停止录制
        ctl.requested.store(false, Ordering::Relaxed);
        rec.tick(&take_snapshot(&mut ctx, 0.5));
        assert!(!ctl.status.lock().unwrap().active, "stop must clear status");

        // 会话应有 ended_at，且能导出报告
        let conn = open_db(&db_path).unwrap();
        let ended: Option<u64> = conn
            .query_row(
                "SELECT ended_at FROM sessions WHERE id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert!(ended.is_some(), "ended_at must be set after stop");
        let report = crate::report::export(&conn, sid, "html", &dir);
        assert!(report.is_ok(), "export failed: {report:?}");
    }

    /// 诊断用：转储真实应用数据库的会话状态（仅手动运行）
    #[test]
    #[ignore = "diag"]
    fn dump_real_db_sessions() {
        let appdata = std::env::var("APPDATA").unwrap();
        let db = std::path::Path::new(&appdata)
            .join("com.luhaishan.sysscope")
            .join("sysscope.db");
        println!("db: {} (exists={})", db.display(), db.exists());
        if !db.exists() {
            return;
        }
        let conn = Connection::open(&db).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.started_at, s.ended_at,
                    (SELECT COUNT(*) FROM samples WHERE session_id = s.id)
                 FROM sessions s ORDER BY s.id DESC LIMIT 15",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .unwrap();
        for row in rows.flatten() {
            println!(
                "session #{}: started={} ended={:?} samples={}",
                row.0, row.1, row.2, row.3
            );
        }
    }

    #[test]
    fn schema_and_session_lifecycle() {
        let dir = std::env::temp_dir().join("sysscope-test-rec");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (db_path, sid) = make_test_db(&dir);

        let conn = open_db(&db_path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM samples WHERE session_id = ?1",
                params![sid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 60);

        // 清理策略不应误删（仅 1 个会话）
        prune_old_sessions(&db_path);
        let sess: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sess, 1);
    }
}
