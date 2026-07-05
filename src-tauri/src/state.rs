//! Shared application state + on-disk paths + API-key storage.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

pub const KEYRING_SERVICE: &str = "com.richikchanda1999.sarvam-stt-app";
pub const KEYRING_USER: &str = "sarvam_api_key";

#[derive(Clone)]
pub struct AppPaths {
    pub audio_dir: PathBuf,
    pub json_dir: PathBuf,
    pub docx_dir: PathBuf,
    pub db_path: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> Result<Self> {
        let base = app
            .path()
            .app_local_data_dir()
            .context("resolving app_local_data_dir")?;
        let paths = Self {
            audio_dir: base.join("audio"),
            json_dir: base.join("json"),
            docx_dir: base.join("docx"),
            db_path: base.join("db").join("app.sqlite3"),
        };
        for dir in [
            &paths.audio_dir,
            &paths.json_dir,
            &paths.docx_dir,
            &paths.db_path.parent().unwrap().to_path_buf(),
        ] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(paths)
    }
}

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub http: reqwest::Client,
    pub paths: AppPaths,
    /// run_id -> cancellation tokens for that run's in-flight job tasks.
    pub run_tokens: Mutex<HashMap<String, Vec<CancellationToken>>>,
}

impl AppState {
    pub fn new(db: Connection, paths: AppPaths) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            http: reqwest::Client::new(),
            paths,
            run_tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn register_token(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.run_tokens
            .lock()
            .unwrap()
            .entry(run_id.to_string())
            .or_default()
            .push(token.clone());
        token
    }

    pub fn cancel_run(&self, run_id: &str) {
        if let Some(tokens) = self.run_tokens.lock().unwrap().get(run_id) {
            for t in tokens {
                t.cancel();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// API key storage: OS keychain, with a SQLite-settings fallback.
// ---------------------------------------------------------------------------

pub fn store_api_key(state: &AppState, key: &str) -> Result<()> {
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(entry) if entry.set_password(key).is_ok() => {
            // Keychain succeeded — make sure no stale fallback lingers.
            let conn = state.db.lock().unwrap();
            let _ = crate::db::delete_setting(&conn, "api_key");
            Ok(())
        }
        _ => {
            // Keychain unavailable (e.g. headless Linux) — fall back to settings table.
            let conn = state.db.lock().unwrap();
            crate::db::set_setting(&conn, "api_key", key)
        }
    }
}

pub fn get_api_key(state: &AppState) -> Option<String> {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        if let Ok(pw) = entry.get_password() {
            if !pw.is_empty() {
                return Some(pw);
            }
        }
    }
    let conn = state.db.lock().unwrap();
    crate::db::get_setting(&conn, "api_key").ok().flatten()
}

pub fn has_api_key(state: &AppState) -> bool {
    get_api_key(state).is_some()
}
