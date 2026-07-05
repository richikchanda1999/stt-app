//! Background job engine. One tokio task per Sarvam job drives
//! create -> upload -> start -> poll -> download -> finalize, persisting to
//! SQLite and emitting `run://*` events. Also handles startup resume.
//!
//! Invariants ported from the Python CLI: persist the job row the instant the
//! job is created (before upload) and never re-create a job that already has a
//! persisted job_id — resume re-attaches via /status.

use crate::db;
use crate::sarvam::{self, JobParameters, JobStatus, Sarvam};
use crate::state::{get_api_key, AppState};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

pub const POLL_INTERVAL_SECS: u64 = 10;
pub const JOB_TIMEOUT_SECS: u64 = 3600;

// ---------------------------------------------------------------------------
// Event payloads
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
struct FileProgress {
    run_id: String,
    run_file_id: String,
    state: String,
    error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
struct JobProgress {
    run_id: String,
    job_id: String,
    job_state: String,
    total: Option<i64>,
    successful: Option<i64>,
    failed: Option<i64>,
}

#[derive(Clone, serde::Serialize)]
struct RunStateEv {
    run_id: String,
    aggregate_state: String,
    total: i64,
    done: i64,
    failed: i64,
}

// ---------------------------------------------------------------------------
// Small helpers (all DB ops keep the lock scope tight — never held across await)
// ---------------------------------------------------------------------------

fn with_db<T>(state: &AppState, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let conn = state.db.lock().unwrap();
    f(&conn)
}

fn client(state: &AppState) -> Option<Sarvam> {
    get_api_key(state).map(|k| Sarvam::new(state.http.clone(), k))
}

fn is_active(state: &str) -> bool {
    matches!(
        state,
        db::F_QUEUED | db::F_UPLOADING | db::F_PROCESSING | db::F_DOWNLOADING
    )
}

fn update_file(app: &AppHandle, state: &AppState, run_id: &str, rf_id: &str, st: &str, err: Option<&str>) {
    let _ = with_db(state, |c| db::set_run_file_state(c, rf_id, st, err));
    let _ = app.emit(
        "run://file-progress",
        FileProgress {
            run_id: run_id.to_string(),
            run_file_id: rf_id.to_string(),
            state: st.to_string(),
            error: err.map(str::to_string),
        },
    );
}

fn emit_run_state(app: &AppHandle, state: &AppState, run_id: &str) {
    if let Ok(s) = with_db(state, |c| db::recompute_aggregate(c, run_id)) {
        let _ = app.emit(
            "run://state",
            RunStateEv {
                run_id: run_id.to_string(),
                aggregate_state: s.aggregate_state,
                total: s.total,
                done: s.done,
                failed: s.failed,
            },
        );
    }
}

fn emit_job(app: &AppHandle, run_id: &str, job_id: &str, job_state: &str, status: Option<&JobStatus>) {
    let _ = app.emit(
        "run://job-progress",
        JobProgress {
            run_id: run_id.to_string(),
            job_id: job_id.to_string(),
            job_state: job_state.to_string(),
            total: status.and_then(|s| s.total_files),
            successful: status.and_then(|s| s.successful_files_count),
            failed: status.and_then(|s| s.failed_files_count),
        },
    );
}

fn mark_active_files(app: &AppHandle, state: &AppState, run_id: &str, job_id: &str, new_state: &str, err: Option<&str>) {
    let rows = with_db(state, |c| db::run_files_for_job(c, job_id)).unwrap_or_default();
    for r in rows {
        if is_active(&r.state) {
            update_file(app, state, run_id, &r.id, new_state, err);
        }
    }
    emit_run_state(app, state, run_id);
}

fn fail_files(app: &AppHandle, state: &AppState, run_id: &str, ids: &[String], err: &str) {
    for id in ids {
        update_file(app, state, run_id, id, db::F_FAILED, Some(err));
    }
    emit_run_state(app, state, run_id);
}

// ---------------------------------------------------------------------------
// Fresh group: initialise -> persist -> upload -> start -> drive
// ---------------------------------------------------------------------------

pub async fn spawn_group(
    app: AppHandle,
    state: Arc<AppState>,
    run_id: String,
    language: String,
    run_file_ids: Vec<String>,
    token: CancellationToken,
) {
    let sarvam = match client(&state) {
        Some(s) => s,
        None => {
            fail_files(&app, &state, &run_id, &run_file_ids, "No Sarvam API key set — open Settings.");
            return;
        }
    };

    let rows = match with_db(&state, |c| {
        run_file_ids.iter().map(|id| db::run_file_row(c, id)).collect::<Result<Vec<_>>>()
    }) {
        Ok(r) => r,
        Err(e) => {
            fail_files(&app, &state, &run_id, &run_file_ids, &e.to_string());
            return;
        }
    };

    let run = match with_db(&state, |c| db::run_summary(c, &run_id)) {
        Ok(r) => r,
        Err(_) => return,
    };

    let params = JobParameters {
        language_code: Some(language.clone()),
        model: run.model.clone(),
        mode: run.mode.clone(),
        with_diarization: run.with_diarization,
        with_timestamps: run.with_timestamps,
        num_speakers: run.num_speakers,
    };

    // initialise (billable job created here)
    let job_id = match sarvam.initialise(params).await {
        Ok(j) => j,
        Err(e) => {
            fail_files(&app, &state, &run_id, &run_file_ids, &format!("create job failed: {e}"));
            return;
        }
    };

    // Persist job + attach files BEFORE upload (crash-recoverable, matches CLI).
    let _ = with_db(&state, |c| {
        db::insert_job(c, &job_id, &run_id, &language)?;
        for r in &rows {
            db::set_run_file_job(c, &r.id, &job_id)?;
        }
        Ok(())
    });
    emit_job(&app, &run_id, &job_id, "Accepted", None);

    if let Err(e) = upload_and_start(&app, &state, &sarvam, &job_id, &run_id, &rows, &token).await {
        let _ = with_db(&state, |c| db::update_job_state(c, &job_id, "failed", Some(&e.to_string())));
        mark_active_files(&app, &state, &run_id, &job_id, db::F_FAILED, Some(&e.to_string()));
        return;
    }

    drive_job(&app, &state, &sarvam, &job_id, &run_id, &token).await;
}

async fn upload_and_start(
    app: &AppHandle,
    state: &AppState,
    sarvam: &Sarvam,
    job_id: &str,
    run_id: &str,
    rows: &[db::RunFileRow],
    token: &CancellationToken,
) -> Result<()> {
    let names: Vec<String> = rows.iter().map(|r| r.upload_name.clone()).collect();
    let links = sarvam.get_upload_links(job_id, &names).await?;

    let mut any_uploaded = false;
    for r in rows {
        if token.is_cancelled() {
            update_file(app, state, run_id, &r.id, db::F_CANCELLED, None);
            continue;
        }
        update_file(app, state, run_id, &r.id, db::F_UPLOADING, None);
        match links.upload_urls.get(&r.upload_name) {
            Some(link) => {
                let ct = sarvam::content_type_for(&r.ext);
                match sarvam.upload_file(&link.file_url, Path::new(&r.stored_path), ct).await {
                    Ok(()) => {
                        any_uploaded = true;
                        update_file(app, state, run_id, &r.id, db::F_PROCESSING, None);
                    }
                    Err(e) => update_file(app, state, run_id, &r.id, db::F_FAILED, Some(&format!("upload failed: {e}"))),
                }
            }
            None => update_file(app, state, run_id, &r.id, db::F_FAILED, Some("no upload URL returned")),
        }
    }
    emit_run_state(app, state, run_id);

    if !any_uploaded {
        anyhow::bail!("no files uploaded successfully");
    }

    sarvam.start(job_id).await?;
    let _ = with_db(state, |c| db::update_job_state(c, job_id, "running", None));
    emit_job(app, run_id, job_id, "Running", None);
    Ok(())
}

// ---------------------------------------------------------------------------
// Poll loop (shared by fresh + resume)
// ---------------------------------------------------------------------------

async fn drive_job(
    app: &AppHandle,
    state: &AppState,
    sarvam: &Sarvam,
    job_id: &str,
    run_id: &str,
    token: &CancellationToken,
) {
    let start = Instant::now();
    loop {
        if token.is_cancelled() {
            mark_active_files(app, state, run_id, job_id, db::F_CANCELLED, None);
            return;
        }

        let status = match sarvam.get_status(job_id).await {
            Ok(s) => s,
            Err(e) => {
                let _ = with_db(state, |c| db::update_job_state(c, job_id, "failed", Some(&e.to_string())));
                mark_active_files(app, state, run_id, job_id, db::F_FAILED, Some(&e.to_string()));
                return;
            }
        };
        let js = status.job_state.to_lowercase();
        emit_job(app, run_id, job_id, &status.job_state, Some(&status));

        match js.as_str() {
            "completed" => {
                finalize(app, state, sarvam, job_id, run_id, &status).await;
                return;
            }
            "failed" => {
                let err = status.error_message.clone().unwrap_or_else(|| "job failed".into());
                let _ = with_db(state, |c| db::update_job_state(c, job_id, "failed", Some(&err)));
                mark_active_files(app, state, run_id, job_id, db::F_FAILED, Some(&err));
                return;
            }
            _ => {
                let _ = with_db(state, |c| db::update_job_state(c, job_id, &js, None));
            }
        }

        if start.elapsed().as_secs() > JOB_TIMEOUT_SECS {
            let _ = with_db(state, |c| db::update_job_state(c, job_id, "failed", Some("timeout")));
            mark_active_files(app, state, run_id, job_id, db::F_FAILED, Some("job timed out"));
            return;
        }

        tokio::select! {
            _ = token.cancelled() => {
                mark_active_files(app, state, run_id, job_id, db::F_CANCELLED, None);
                return;
            }
            _ = tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Finalize a completed job: download outputs + convert per-file states
// ---------------------------------------------------------------------------

async fn finalize(
    app: &AppHandle,
    state: &AppState,
    sarvam: &Sarvam,
    job_id: &str,
    run_id: &str,
    status: &JobStatus,
) {
    let rows = with_db(state, |c| db::run_files_for_job(c, job_id)).unwrap_or_default();

    // Index job_details by input file name.
    let mut by_input: HashMap<String, sarvam::JobDetail> = HashMap::new();
    for d in status.job_details.clone().unwrap_or_default() {
        if let Some(inp) = d.inputs.first() {
            by_input.insert(inp.file_name.clone(), d);
        }
    }

    // Plan downloads for successes; fail the rest now.
    let mut output_names: Vec<String> = Vec::new();
    let mut plan: Vec<(db::RunFileRow, String)> = Vec::new();
    for r in &rows {
        match by_input.get(&r.upload_name) {
            Some(d) if d.state == "Success" => match d.outputs.first() {
                Some(out) => {
                    output_names.push(out.file_name.clone());
                    plan.push((r.clone(), out.file_name.clone()));
                }
                None => update_file(app, state, run_id, &r.id, db::F_FAILED, Some("no output produced")),
            },
            Some(d) => {
                let e = d.error_message.clone().unwrap_or_else(|| "file failed".into());
                update_file(app, state, run_id, &r.id, db::F_FAILED, Some(&e));
            }
            None => {
                // Not failed here if the file simply never uploaded (already failed).
                if is_active(&r.state) {
                    update_file(app, state, run_id, &r.id, db::F_FAILED, Some("no result for file"));
                }
            }
        }
    }

    if !output_names.is_empty() {
        match sarvam.get_download_links(job_id, &output_names).await {
            Ok(links) => {
                for (r, out) in &plan {
                    update_file(app, state, run_id, &r.id, db::F_DOWNLOADING, None);
                    let url = links.download_urls.get(out).map(|s| s.file_url.clone());
                    match url {
                        Some(u) => match sarvam.download_bytes(&u).await {
                            Ok(bytes) => {
                                let path = state
                                    .paths
                                    .json_dir
                                    .join(run_id)
                                    .join(format!("{}.json", r.upload_name));
                                if let Some(p) = path.parent() {
                                    let _ = std::fs::create_dir_all(p);
                                }
                                if let Err(e) = std::fs::write(&path, &bytes) {
                                    update_file(app, state, run_id, &r.id, db::F_FAILED, Some(&format!("write failed: {e}")));
                                    continue;
                                }
                                let _ = with_db(state, |c| {
                                    db::set_run_file_transcript(c, &r.id, &path.to_string_lossy())
                                });
                                update_file(app, state, run_id, &r.id, db::F_DONE, None);
                            }
                            Err(e) => update_file(app, state, run_id, &r.id, db::F_FAILED, Some(&format!("download failed: {e}"))),
                        },
                        None => update_file(app, state, run_id, &r.id, db::F_FAILED, Some("no download URL")),
                    }
                }
            }
            Err(e) => {
                for (r, _) in &plan {
                    update_file(app, state, run_id, &r.id, db::F_FAILED, Some(&format!("download links failed: {e}")));
                }
            }
        }
    }

    let _ = with_db(state, |c| db::update_job_state(c, job_id, "completed", None));
    emit_run_state(app, state, run_id);
}

// ---------------------------------------------------------------------------
// Resume an existing job by job_id (startup + retained tokens)
// ---------------------------------------------------------------------------

pub async fn spawn_resume(
    app: AppHandle,
    state: Arc<AppState>,
    job_id: String,
    run_id: String,
    token: CancellationToken,
) {
    let sarvam = match client(&state) {
        Some(s) => s,
        None => return,
    };
    let status = match sarvam.get_status(&job_id).await {
        Ok(s) => s,
        Err(_) => return, // leave as-is; a later launch can retry
    };
    let js = status.job_state.to_lowercase();
    emit_job(&app, &run_id, &job_id, &status.job_state, Some(&status));

    match js.as_str() {
        "completed" => finalize(&app, &state, &sarvam, &job_id, &run_id, &status).await,
        "failed" => {
            let err = status.error_message.clone().unwrap_or_else(|| "job failed".into());
            let _ = with_db(&state, |c| db::update_job_state(c, &job_id, "failed", Some(&err)));
            mark_active_files(&app, &state, &run_id, &job_id, db::F_FAILED, Some(&err));
        }
        "pending" | "running" => drive_job(&app, &state, &sarvam, &job_id, &run_id, &token).await,
        _ => {
            // Accepted: never started -> safe to (re-)upload + start (overwrite-safe).
            let rows = with_db(&state, |c| db::run_files_for_job(c, &job_id)).unwrap_or_default();
            if let Err(e) = upload_and_start(&app, &state, &sarvam, &job_id, &run_id, &rows, &token).await {
                let _ = with_db(&state, |c| db::update_job_state(c, &job_id, "failed", Some(&e.to_string())));
                mark_active_files(&app, &state, &run_id, &job_id, db::F_FAILED, Some(&e.to_string()));
                return;
            }
            drive_job(&app, &state, &sarvam, &job_id, &run_id, &token).await;
        }
    }
}
