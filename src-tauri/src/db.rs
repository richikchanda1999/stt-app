//! SQLite persistence: assets, runs, jobs, run_files, settings.
//! Connection lives behind a Mutex in app state; all functions take `&Connection`
//! and keep lock scopes short (no `.await` while holding the lock).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

// Per-file lifecycle states (rendered with loaders in the UI).
pub const F_QUEUED: &str = "queued";
pub const F_UPLOADING: &str = "uploading";
pub const F_PROCESSING: &str = "processing";
pub const F_DOWNLOADING: &str = "downloading";
pub const F_DONE: &str = "done";
pub const F_FAILED: &str = "failed";
pub const F_CANCELLED: &str = "cancelled";

// ---------------------------------------------------------------------------
// DTOs returned to the frontend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetDto {
    pub id: String,
    pub sha256: String,
    pub original_name: String,
    pub ext: String,
    pub size_bytes: i64,
    pub stored_path: String,
    pub imported_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunFileDto {
    pub id: String,
    pub asset_id: String,
    pub original_name: String,
    pub job_id: Option<String>,
    pub effective_language: String,
    pub state: String,
    pub error: Option<String>,
    pub has_transcript: bool,
    pub docx_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunSummaryDto {
    pub id: String,
    pub created_at: String,
    pub model: String,
    pub mode: Option<String>,
    pub default_language: String,
    pub with_diarization: bool,
    pub with_timestamps: bool,
    pub num_speakers: Option<i64>,
    pub aggregate_state: String,
    pub parent_run_id: Option<String>,
    pub total: i64,
    pub done: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunDetailDto {
    pub run: RunSummaryDto,
    pub files: Vec<RunFileDto>,
}

// Parameters captured when a run is created.
#[derive(Debug, Clone)]
pub struct RunParams {
    pub default_language: String,
    pub model: String,
    pub mode: Option<String>,
    pub with_diarization: bool,
    pub with_timestamps: bool,
    pub num_speakers: Option<i64>,
    pub parent_run_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

pub fn init(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS assets (
            id            TEXT PRIMARY KEY,
            sha256        TEXT NOT NULL UNIQUE,
            original_name TEXT NOT NULL,
            stored_path   TEXT NOT NULL,
            ext           TEXT NOT NULL,
            size_bytes    INTEGER NOT NULL,
            imported_at   TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runs (
            id               TEXT PRIMARY KEY,
            created_at       TEXT NOT NULL,
            default_language TEXT NOT NULL,
            model            TEXT NOT NULL,
            mode             TEXT,
            with_diarization INTEGER NOT NULL DEFAULT 1,
            with_timestamps  INTEGER NOT NULL DEFAULT 1,
            num_speakers     INTEGER,
            aggregate_state  TEXT NOT NULL,
            parent_run_id    TEXT REFERENCES runs(id)
        );

        CREATE TABLE IF NOT EXISTS jobs (
            id            TEXT PRIMARY KEY,
            run_id        TEXT NOT NULL REFERENCES runs(id),
            language      TEXT NOT NULL,
            state         TEXT NOT NULL,
            error_message TEXT,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS run_files (
            id                   TEXT PRIMARY KEY,
            run_id               TEXT NOT NULL REFERENCES runs(id),
            asset_id             TEXT NOT NULL REFERENCES assets(id),
            job_id               TEXT REFERENCES jobs(id),
            effective_language   TEXT NOT NULL,
            state                TEXT NOT NULL,
            error                TEXT,
            transcript_json_path TEXT,
            docx_path            TEXT,
            upload_name          TEXT NOT NULL,
            updated_at           TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_run_files_run  ON run_files(run_id);
        CREATE INDEX IF NOT EXISTS idx_run_files_job  ON run_files(job_id);
        CREATE INDEX IF NOT EXISTS idx_jobs_run       ON jobs(run_id);
        CREATE INDEX IF NOT EXISTS idx_jobs_state     ON jobs(state);
        "#,
    )?;
    Ok(())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Assets (content-addressed, dedup by sha256)
// ---------------------------------------------------------------------------

pub fn find_asset_by_sha(conn: &Connection, sha: &str) -> Result<Option<AssetDto>> {
    Ok(conn
        .query_row(
            "SELECT id, sha256, original_name, ext, size_bytes, stored_path, imported_at
             FROM assets WHERE sha256 = ?1",
            [sha],
            map_asset,
        )
        .optional()?)
}

pub fn insert_asset(conn: &Connection, a: &AssetDto) -> Result<()> {
    conn.execute(
        "INSERT INTO assets(id, sha256, original_name, stored_path, ext, size_bytes, imported_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            a.id,
            a.sha256,
            a.original_name,
            a.stored_path,
            a.ext,
            a.size_bytes,
            a.imported_at
        ],
    )?;
    Ok(())
}

pub fn get_asset(conn: &Connection, id: &str) -> Result<AssetDto> {
    Ok(conn.query_row(
        "SELECT id, sha256, original_name, ext, size_bytes, stored_path, imported_at
         FROM assets WHERE id = ?1",
        [id],
        map_asset,
    )?)
}

fn map_asset(r: &rusqlite::Row) -> rusqlite::Result<AssetDto> {
    Ok(AssetDto {
        id: r.get(0)?,
        sha256: r.get(1)?,
        original_name: r.get(2)?,
        ext: r.get(3)?,
        size_bytes: r.get(4)?,
        stored_path: r.get(5)?,
        imported_at: r.get(6)?,
    })
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

pub fn insert_run(conn: &Connection, id: &str, p: &RunParams) -> Result<()> {
    conn.execute(
        "INSERT INTO runs(id, created_at, default_language, model, mode,
                          with_diarization, with_timestamps, num_speakers,
                          aggregate_state, parent_run_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            id,
            now(),
            p.default_language,
            p.model,
            p.mode,
            p.with_diarization as i64,
            p.with_timestamps as i64,
            p.num_speakers,
            "queued",
            p.parent_run_id,
        ],
    )?;
    Ok(())
}

pub fn run_summary(conn: &Connection, run_id: &str) -> Result<RunSummaryDto> {
    let mut s = conn.query_row(
        "SELECT id, created_at, model, mode, default_language,
                with_diarization, with_timestamps, num_speakers, aggregate_state, parent_run_id
         FROM runs WHERE id = ?1",
        [run_id],
        |r| {
            Ok(RunSummaryDto {
                id: r.get(0)?,
                created_at: r.get(1)?,
                model: r.get(2)?,
                mode: r.get(3)?,
                default_language: r.get(4)?,
                with_diarization: r.get::<_, i64>(5)? != 0,
                with_timestamps: r.get::<_, i64>(6)? != 0,
                num_speakers: r.get(7)?,
                aggregate_state: r.get(8)?,
                parent_run_id: r.get(9)?,
                total: 0,
                done: 0,
                failed: 0,
            })
        },
    )?;
    let (total, done, failed) = run_counts(conn, run_id)?;
    s.total = total;
    s.done = done;
    s.failed = failed;
    Ok(s)
}

fn run_counts(conn: &Connection, run_id: &str) -> Result<(i64, i64, i64)> {
    Ok(conn.query_row(
        "SELECT
            COUNT(*),
            COALESCE(SUM(CASE WHEN state='done' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN state='failed' THEN 1 ELSE 0 END),0)
         FROM run_files WHERE run_id = ?1",
        [run_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?)
}

pub fn list_runs(conn: &Connection) -> Result<Vec<RunSummaryDto>> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM runs ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    ids.iter().map(|id| run_summary(conn, id)).collect()
}

pub fn run_detail(conn: &Connection, run_id: &str) -> Result<RunDetailDto> {
    let run = run_summary(conn, run_id)?;
    let files = run_files_for_run(conn, run_id)?;
    Ok(RunDetailDto { run, files })
}

fn map_run_file(r: &rusqlite::Row) -> rusqlite::Result<RunFileDto> {
    let transcript_path: Option<String> = r.get(7)?;
    Ok(RunFileDto {
        id: r.get(0)?,
        asset_id: r.get(1)?,
        original_name: r.get(2)?,
        job_id: r.get(3)?,
        effective_language: r.get(4)?,
        state: r.get(5)?,
        error: r.get(6)?,
        has_transcript: transcript_path.is_some(),
        docx_path: r.get(8)?,
    })
}

const RUN_FILE_SELECT: &str = "SELECT rf.id, rf.asset_id, a.original_name, rf.job_id,
            rf.effective_language, rf.state, rf.error, rf.transcript_json_path, rf.docx_path
     FROM run_files rf JOIN assets a ON a.id = rf.asset_id";

pub fn run_files_for_run(conn: &Connection, run_id: &str) -> Result<Vec<RunFileDto>> {
    let mut stmt = conn.prepare(&format!(
        "{RUN_FILE_SELECT} WHERE rf.run_id = ?1 ORDER BY a.original_name"
    ))?;
    let rows = stmt.query_map([run_id], map_run_file)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

pub fn insert_job(conn: &Connection, job_id: &str, run_id: &str, language: &str) -> Result<()> {
    let ts = now();
    conn.execute(
        "INSERT INTO jobs(id, run_id, language, state, created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?5)",
        params![job_id, run_id, language, "accepted", ts],
    )?;
    Ok(())
}

pub fn update_job_state(
    conn: &Connection,
    job_id: &str,
    state: &str,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE jobs SET state=?2, error_message=?3, updated_at=?4 WHERE id=?1",
        params![job_id, state, error, now()],
    )?;
    Ok(())
}

/// (job_id, run_id, language) for every job not in a terminal state — used on startup resume.
pub fn nonterminal_jobs(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, language FROM jobs WHERE state NOT IN ('completed','failed','cancelled')",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// run_files
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn insert_run_file(
    conn: &Connection,
    id: &str,
    run_id: &str,
    asset_id: &str,
    effective_language: &str,
    upload_name: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO run_files(id, run_id, asset_id, job_id, effective_language,
                               state, upload_name, updated_at)
         VALUES(?1,?2,?3,NULL,?4,?5,?6,?7)",
        params![
            id,
            run_id,
            asset_id,
            effective_language,
            F_QUEUED,
            upload_name,
            now()
        ],
    )?;
    Ok(())
}

pub fn set_run_file_job(conn: &Connection, run_file_id: &str, job_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE run_files SET job_id=?2, updated_at=?3 WHERE id=?1",
        params![run_file_id, job_id, now()],
    )?;
    Ok(())
}

pub fn set_run_file_state(
    conn: &Connection,
    run_file_id: &str,
    state: &str,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE run_files SET state=?2, error=?3, updated_at=?4 WHERE id=?1",
        params![run_file_id, state, error, now()],
    )?;
    Ok(())
}

pub fn set_run_file_transcript(conn: &Connection, run_file_id: &str, path: &str) -> Result<()> {
    conn.execute(
        "UPDATE run_files SET transcript_json_path=?2, updated_at=?3 WHERE id=?1",
        params![run_file_id, path, now()],
    )?;
    Ok(())
}

pub fn set_run_file_docx(conn: &Connection, run_file_id: &str, path: &str) -> Result<()> {
    conn.execute(
        "UPDATE run_files SET docx_path=?2, updated_at=?3 WHERE id=?1",
        params![run_file_id, path, now()],
    )?;
    Ok(())
}

/// Full row info needed by the job runner for one run_file.
#[allow(dead_code)] // some fields are carried for completeness / future use
#[derive(Debug, Clone)]
pub struct RunFileRow {
    pub id: String,
    pub run_id: String,
    pub asset_id: String,
    pub upload_name: String,
    pub stored_path: String,
    pub ext: String,
    pub state: String,
    pub transcript_json_path: Option<String>,
    pub docx_path: Option<String>,
    pub original_name: String,
}

fn map_run_file_row(r: &rusqlite::Row) -> rusqlite::Result<RunFileRow> {
    Ok(RunFileRow {
        id: r.get(0)?,
        run_id: r.get(1)?,
        asset_id: r.get(2)?,
        upload_name: r.get(3)?,
        stored_path: r.get(4)?,
        ext: r.get(5)?,
        state: r.get(6)?,
        transcript_json_path: r.get(7)?,
        docx_path: r.get(8)?,
        original_name: r.get(9)?,
    })
}

const RUN_FILE_ROW_SELECT: &str = "SELECT rf.id, rf.run_id, rf.asset_id, rf.upload_name,
            a.stored_path, a.ext, rf.state, rf.transcript_json_path, rf.docx_path, a.original_name
     FROM run_files rf JOIN assets a ON a.id = rf.asset_id";

pub fn run_files_for_job(conn: &Connection, job_id: &str) -> Result<Vec<RunFileRow>> {
    let mut stmt = conn.prepare(&format!("{RUN_FILE_ROW_SELECT} WHERE rf.job_id = ?1"))?;
    let rows = stmt.query_map([job_id], map_run_file_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn run_file_row(conn: &Connection, run_file_id: &str) -> Result<RunFileRow> {
    Ok(conn.query_row(
        &format!("{RUN_FILE_ROW_SELECT} WHERE rf.id = ?1"),
        [run_file_id],
        map_run_file_row,
    )?)
}

// ---------------------------------------------------------------------------
// Aggregate run state (pure function of run_files.state)
// ---------------------------------------------------------------------------

pub fn recompute_aggregate(conn: &Connection, run_id: &str) -> Result<RunSummaryDto> {
    let states: Vec<String> = {
        let mut stmt = conn.prepare("SELECT state FROM run_files WHERE run_id=?1")?;
        let rows = stmt.query_map([run_id], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let (mut done, mut failed, mut cancelled, mut active) = (0, 0, 0, 0);
    for s in &states {
        match s.as_str() {
            F_DONE => done += 1,
            F_FAILED => failed += 1,
            F_CANCELLED => cancelled += 1,
            _ => active += 1,
        }
    }
    let total = states.len() as i64;
    let agg = if active > 0 {
        "running"
    } else if total > 0 && done == total {
        "completed"
    } else if done > 0 {
        "partial"
    } else if failed > 0 && cancelled == 0 {
        "failed"
    } else if cancelled > 0 && failed == 0 {
        "cancelled"
    } else if failed > 0 || cancelled > 0 {
        "partial"
    } else {
        "queued"
    };
    conn.execute(
        "UPDATE runs SET aggregate_state=?2 WHERE id=?1",
        params![run_id, agg],
    )?;
    run_summary(conn, run_id)
}
