mod db;
mod docx;
mod error;
mod runner;
mod sarvam;
mod state;

use error::{err, AppResult};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use state::{AppPaths, AppState};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// Command input/output DTOs
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct SettingsDto {
    has_api_key: bool,
    default_language: String,
    model: String,
    mode: Option<String>,
    with_diarization: bool,
    with_timestamps: bool,
    num_speakers: Option<i64>,
}

#[derive(serde::Deserialize)]
struct SettingsInput {
    api_key: Option<String>,
    default_language: String,
    model: String,
    mode: Option<String>,
    with_diarization: bool,
    with_timestamps: bool,
    num_speakers: Option<i64>,
}

#[derive(serde::Deserialize)]
struct FileSel {
    asset_id: String,
    language: Option<String>,
}

#[derive(serde::Deserialize)]
struct StartRunInput {
    files: Vec<FileSel>,
    default_language: String,
    model: String,
    mode: Option<String>,
    with_diarization: bool,
    with_timestamps: bool,
    num_speakers: Option<i64>,
}

#[derive(serde::Deserialize)]
struct RetryInput {
    language: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    with_diarization: Option<bool>,
    with_timestamps: Option<bool>,
    num_speakers: Option<i64>,
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn get_pref(state: &AppState, key: &str, default: &str) -> String {
    let conn = state.db.lock().unwrap();
    db::get_setting(&conn, key).ok().flatten().unwrap_or_else(|| default.to_string())
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> AppResult<SettingsDto> {
    let num = {
        let conn = state.db.lock().unwrap();
        db::get_setting(&conn, "num_speakers").ok().flatten()
    };
    Ok(SettingsDto {
        has_api_key: state::has_api_key(&state),
        default_language: get_pref(&state, "default_language", "unknown"),
        model: get_pref(&state, "model", "saaras:v3"),
        mode: Some(get_pref(&state, "mode", "transcribe")),
        with_diarization: get_pref(&state, "with_diarization", "1") == "1",
        with_timestamps: get_pref(&state, "with_timestamps", "1") == "1",
        num_speakers: num.and_then(|s| s.parse().ok()),
    })
}

#[tauri::command]
fn set_settings(state: State<'_, Arc<AppState>>, input: SettingsInput) -> AppResult<()> {
    if let Some(key) = input.api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        state::store_api_key(&state, key.trim())?;
    }
    let conn = state.db.lock().unwrap();
    db::set_setting(&conn, "default_language", &input.default_language)?;
    db::set_setting(&conn, "model", &input.model)?;
    db::set_setting(&conn, "mode", input.mode.as_deref().unwrap_or("transcribe"))?;
    db::set_setting(&conn, "with_diarization", if input.with_diarization { "1" } else { "0" })?;
    db::set_setting(&conn, "with_timestamps", if input.with_timestamps { "1" } else { "0" })?;
    match input.num_speakers {
        Some(n) => db::set_setting(&conn, "num_speakers", &n.to_string())?,
        None => db::delete_setting(&conn, "num_speakers")?,
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

fn sha256_file(path: &PathBuf) -> AppResult<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    // Heap buffer, NOT a stack array: a 1 MiB stack allocation overflows the
    // default 1 MB thread stack on Windows and crashes the app.
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[tauri::command]
fn import_files(state: State<'_, Arc<AppState>>, paths: Vec<String>) -> AppResult<Vec<db::AssetDto>> {
    let mut out = Vec::new();
    for p in paths {
        let path = PathBuf::from(&p);
        if !path.is_file() {
            continue;
        }
        let original_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "audio".into());
        let ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let sha = sha256_file(&path)?;

        // Dedup by content.
        if let Some(existing) = {
            let conn = state.db.lock().unwrap();
            db::find_asset_by_sha(&conn, &sha)?
        } {
            out.push(existing);
            continue;
        }

        let size = std::fs::metadata(&path)?.len() as i64;
        let stored = state.paths.audio_dir.join(if ext.is_empty() {
            sha.clone()
        } else {
            format!("{sha}.{ext}")
        });
        std::fs::copy(&path, &stored)?;

        let asset = db::AssetDto {
            id: uuid::Uuid::new_v4().to_string(),
            sha256: sha,
            original_name,
            ext,
            size_bytes: size,
            stored_path: stored.to_string_lossy().to_string(),
            imported_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        {
            let conn = state.db.lock().unwrap();
            db::insert_asset(&conn, &asset)?;
        }
        out.push(asset);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_runs(state: State<'_, Arc<AppState>>) -> AppResult<Vec<db::RunSummaryDto>> {
    let conn = state.db.lock().unwrap();
    Ok(db::list_runs(&conn)?)
}

#[tauri::command]
fn get_run_detail(state: State<'_, Arc<AppState>>, run_id: String) -> AppResult<db::RunDetailDto> {
    let conn = state.db.lock().unwrap();
    Ok(db::run_detail(&conn, &run_id)?)
}

#[tauri::command]
fn get_transcript(state: State<'_, Arc<AppState>>, run_file_id: String) -> AppResult<serde_json::Value> {
    let path = {
        let conn = state.db.lock().unwrap();
        db::run_file_row(&conn, &run_file_id)?.transcript_json_path
    };
    match path {
        Some(p) => {
            let bytes = std::fs::read(&p)?;
            Ok(serde_json::from_slice(&bytes)?)
        }
        None => err("no transcript available for this file"),
    }
}

/// Sanitize + prefix a filename so upload names are unique within a job.
fn upload_name_for(rf_id: &str, original_name: &str) -> String {
    let safe: String = original_name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    format!("{}_{}", &rf_id[..8], safe)
}

/// Create run_files rows, group by language, chunk <=20, and spawn a task per group.
fn launch_run(
    app: &AppHandle,
    state: &Arc<AppState>,
    run_id: &str,
    files: &[(String, String)], // (asset_id, effective_language)
) -> AppResult<()> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    {
        let conn = state.db.lock().unwrap();
        for (asset_id, language) in files {
            let asset = db::get_asset(&conn, asset_id)?;
            let rf_id = uuid::Uuid::new_v4().to_string();
            let upload_name = upload_name_for(&rf_id, &asset.original_name);
            db::insert_run_file(&conn, &rf_id, run_id, asset_id, language, &upload_name)?;
            groups.entry(language.clone()).or_default().push(rf_id);
        }
    }

    for (language, ids) in groups {
        for chunk in ids.chunks(20) {
            let token = state.register_token(run_id);
            let app2 = app.clone();
            let st2 = Arc::clone(state);
            let run2 = run_id.to_string();
            let lang2 = language.clone();
            let ids_v = chunk.to_vec();
            tauri::async_runtime::spawn(async move {
                runner::spawn_group(app2, st2, run2, lang2, ids_v, token).await;
            });
        }
    }
    emit_run_state_now(app, state, run_id);
    Ok(())
}

fn emit_run_state_now(app: &AppHandle, state: &Arc<AppState>, run_id: &str) {
    if let Ok(s) = {
        let conn = state.db.lock().unwrap();
        db::recompute_aggregate(&conn, run_id)
    } {
        let _ = app.emit(
            "run://state",
            serde_json::json!({
                "run_id": run_id,
                "aggregate_state": s.aggregate_state,
                "total": s.total,
                "done": s.done,
                "failed": s.failed,
            }),
        );
    }
}

#[tauri::command]
fn start_run(app: AppHandle, state: State<'_, Arc<AppState>>, input: StartRunInput) -> AppResult<String> {
    if input.files.is_empty() {
        return err("no files selected");
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let params = db::RunParams {
        default_language: input.default_language.clone(),
        model: input.model.clone(),
        mode: input.mode.clone(),
        with_diarization: input.with_diarization,
        with_timestamps: input.with_timestamps,
        num_speakers: input.num_speakers,
        parent_run_id: None,
    };
    {
        let conn = state.db.lock().unwrap();
        db::insert_run(&conn, &run_id, &params)?;
    }

    let files: Vec<(String, String)> = input
        .files
        .into_iter()
        .map(|f| {
            let lang = f.language.unwrap_or_else(|| input.default_language.clone());
            (f.asset_id, lang)
        })
        .collect();

    let state_arc = Arc::clone(&state);
    launch_run(&app, &state_arc, &run_id, &files)?;
    Ok(run_id)
}

#[tauri::command]
fn retry_failed(app: AppHandle, state: State<'_, Arc<AppState>>, run_id: String, overrides: RetryInput) -> AppResult<String> {
    // Gather failed files of the source run.
    let (orig, failed_files) = {
        let conn = state.db.lock().unwrap();
        let orig = db::run_summary(&conn, &run_id)?;
        let files: Vec<(String, String)> = db::run_files_for_run(&conn, &run_id)?
            .into_iter()
            .filter(|f| f.state == "failed")
            .map(|f| (f.asset_id, f.effective_language))
            .collect();
        (orig, files)
    };
    if failed_files.is_empty() {
        return err("no failed files to retry");
    }

    let new_run_id = uuid::Uuid::new_v4().to_string();
    let params = db::RunParams {
        default_language: overrides.language.clone().unwrap_or(orig.default_language.clone()),
        model: overrides.model.clone().unwrap_or(orig.model.clone()),
        mode: overrides.mode.clone().or(orig.mode.clone()),
        with_diarization: overrides.with_diarization.unwrap_or(orig.with_diarization),
        with_timestamps: overrides.with_timestamps.unwrap_or(orig.with_timestamps),
        num_speakers: overrides.num_speakers.or(orig.num_speakers),
        parent_run_id: Some(run_id.clone()),
    };
    {
        let conn = state.db.lock().unwrap();
        db::insert_run(&conn, &new_run_id, &params)?;
    }

    // If a language override is given, apply it to all retried files.
    let files: Vec<(String, String)> = failed_files
        .into_iter()
        .map(|(asset_id, lang)| (asset_id, overrides.language.clone().unwrap_or(lang)))
        .collect();

    let state_arc = Arc::clone(&state);
    launch_run(&app, &state_arc, &new_run_id, &files)?;
    Ok(new_run_id)
}

#[tauri::command]
fn cancel_run(state: State<'_, Arc<AppState>>, run_id: String) -> AppResult<()> {
    state.cancel_run(&run_id);
    Ok(())
}

#[tauri::command]
fn export_docx(state: State<'_, Arc<AppState>>, run_file_id: String) -> AppResult<String> {
    let (row, model) = {
        let conn = state.db.lock().unwrap();
        let row = db::run_file_row(&conn, &run_file_id)?;
        let run = db::run_summary(&conn, &row.run_id)?;
        (row, run.model)
    };
    let json_path = match &row.transcript_json_path {
        Some(p) => p.clone(),
        None => return err("no transcript to export"),
    };
    let bytes = std::fs::read(&json_path)?;

    let stem = std::path::Path::new(&row.original_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| row.original_name.clone());
    let out_path = state.paths.docx_dir.join(&row.run_id).join(format!("{stem}.docx"));
    let generated = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    docx::json_to_docx(&bytes, &row.original_name, &model, &generated, &out_path)?;

    let out_str = out_path.to_string_lossy().to_string();
    {
        let conn = state.db.lock().unwrap();
        db::set_run_file_docx(&conn, &run_file_id, &out_str)?;
    }
    Ok(out_str)
}

// ---------------------------------------------------------------------------
// Update check — compare the running version against the latest GitHub release
// ---------------------------------------------------------------------------

const RELEASES_REPO: &str = "richikchanda1999/stt-app";

#[derive(serde::Serialize)]
struct UpdateInfo {
    version: String,
    current: String,
    url: String,
}

fn parse_ver(s: &str) -> Vec<u64> {
    s.split('.')
        .map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let (a, b) = (parse_ver(candidate), parse_ver(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[tauri::command]
async fn check_for_update(state: State<'_, Arc<AppState>>) -> AppResult<Option<UpdateInfo>> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let resp = state
        .http
        .get(format!("https://api.github.com/repos/{RELEASES_REPO}/releases/latest"))
        .header("User-Agent", "sarvam-stt-app")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    // No published release yet, offline, or rate-limited — stay quiet.
    if !resp.status().is_success() {
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    let latest = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or_default().trim_start_matches('v');
    let url = v
        .get("html_url")
        .and_then(|t| t.as_str())
        .unwrap_or("https://github.com/richikchanda1999/stt-app/releases")
        .to_string();
    if !latest.is_empty() && is_newer(latest, &current) {
        Ok(Some(UpdateInfo { version: latest.to_string(), current, url }))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// App bootstrap
// ---------------------------------------------------------------------------

/// Write Rust panics (with a backtrace) to a log file so production crashes on
/// Windows/macOS/Linux leave a readable trail instead of a silent quit.
/// (Note: a stack overflow is a hardware fault, not a panic, so it won't reach
/// this — hence we also avoid large stack buffers.)
fn install_panic_logger(log_path: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let ts = chrono::Utc::now().to_rfc3339();
        let entry = format!("\n===== panic @ {ts} =====\n{info}\n{bt}\n");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            let _ = f.write_all(entry.as_bytes());
        }
        default(info);
    }));
}

fn resume_inflight(app: &AppHandle, state: &Arc<AppState>) {
    let jobs = {
        let conn = state.db.lock().unwrap();
        db::nonterminal_jobs(&conn).unwrap_or_default()
    };
    for (job_id, run_id, _language) in jobs {
        let token = state.register_token(&run_id);
        let app2 = app.clone();
        let st2 = Arc::clone(state);
        tauri::async_runtime::spawn(async move {
            runner::spawn_resume(app2, st2, job_id, run_id, token).await;
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let handle = app.handle();
            let paths = AppPaths::resolve(handle)?;
            install_panic_logger(paths.logs_dir.join("panic.log"));
            let conn = Connection::open(&paths.db_path)?;
            db::init(&conn)?;
            let state = Arc::new(AppState::new(conn, paths));
            app.manage(Arc::clone(&state));

            // Resume any jobs left in-flight by a previous session.
            resume_inflight(handle, &state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            import_files,
            list_runs,
            get_run_detail,
            get_transcript,
            start_run,
            retry_failed,
            cancel_run,
            export_docx,
            check_for_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
