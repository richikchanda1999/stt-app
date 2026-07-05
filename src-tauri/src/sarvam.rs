//! Pure-Rust Sarvam batch Speech-to-Text client (ported from the Python CLI).
//!
//! Flow: initialise -> upload (to Azure presigned URLs) -> start -> poll status
//! -> download outputs. The `api-subscription-key` header is sent ONLY to
//! api.sarvam.ai, never to the Azure presigned URLs.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;

const BASE: &str = "https://api.sarvam.ai";
const MAX_ATTEMPTS: u32 = 5;
const BASE_DELAY_SECS: f64 = 2.0;

// ---------------------------------------------------------------------------
// Wire models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub with_diarization: bool,
    pub with_timestamps: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_speakers: Option<i64>,
}

#[derive(serde::Serialize)]
struct InitBody {
    job_parameters: JobParameters,
    callback: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct InitResponse {
    pub job_id: String,
}

#[derive(serde::Serialize)]
struct FilesBody {
    job_id: String,
    files: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SignedUrl {
    pub file_url: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct UploadResponse {
    pub upload_urls: HashMap<String, SignedUrl>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DownloadResponse {
    pub download_urls: HashMap<String, SignedUrl>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FileRef {
    pub file_name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct JobDetail {
    #[serde(default)]
    pub inputs: Vec<FileRef>,
    #[serde(default)]
    pub outputs: Vec<FileRef>,
    pub state: String,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct JobStatus {
    pub job_state: String,
    #[serde(default)]
    pub total_files: Option<i64>,
    #[serde(default)]
    pub successful_files_count: Option<i64>,
    #[serde(default)]
    pub failed_files_count: Option<i64>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub job_details: Option<Vec<JobDetail>>,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Sarvam {
    http: reqwest::Client,
    key: String,
}

impl Sarvam {
    pub fn new(http: reqwest::Client, key: String) -> Self {
        Self { http, key }
    }

    pub async fn initialise(&self, params: JobParameters) -> Result<String> {
        let body = InitBody {
            job_parameters: params,
            callback: None,
        };
        let resp: InitResponse = with_retry("initialise", || async {
            let r = self
                .http
                .post(format!("{BASE}/speech-to-text/job/v1"))
                .header("api-subscription-key", &self.key)
                .json(&body)
                .send()
                .await?;
            json_ok(r, "initialise").await
        })
        .await?;
        Ok(resp.job_id)
    }

    pub async fn get_upload_links(&self, job_id: &str, files: &[String]) -> Result<UploadResponse> {
        let body = FilesBody {
            job_id: job_id.to_string(),
            files: files.to_vec(),
        };
        with_retry("upload-links", || async {
            let r = self
                .http
                .post(format!("{BASE}/speech-to-text/job/v1/upload-files"))
                .header("api-subscription-key", &self.key)
                .json(&body)
                .send()
                .await?;
            json_ok(r, "upload-links").await
        })
        .await
    }

    /// PUT a local file to its Azure presigned URL (no auth header on Azure).
    pub async fn upload_file(&self, url: &str, path: &Path, content_type: &str) -> Result<()> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        with_retry("upload-put", || {
            let bytes = bytes.clone();
            async move {
                let r = self
                    .http
                    .put(url)
                    .header("x-ms-blob-type", "BlockBlob")
                    .header("Content-Type", content_type)
                    .body(bytes)
                    .send()
                    .await?;
                if r.status().is_success() {
                    Ok(())
                } else {
                    Err(anyhow!("upload PUT failed: {}", r.status()))
                }
            }
        })
        .await
    }

    pub async fn start(&self, job_id: &str) -> Result<JobStatus> {
        with_retry("start", || async {
            let r = self
                .http
                .post(format!("{BASE}/speech-to-text/job/v1/{job_id}/start"))
                .header("api-subscription-key", &self.key)
                .send()
                .await?;
            json_ok(r, "start").await
        })
        .await
    }

    pub async fn get_status(&self, job_id: &str) -> Result<JobStatus> {
        with_retry("status", || async {
            let r = self
                .http
                .get(format!("{BASE}/speech-to-text/job/v1/{job_id}/status"))
                .header("api-subscription-key", &self.key)
                .send()
                .await?;
            json_ok(r, "status").await
        })
        .await
    }

    pub async fn get_download_links(
        &self,
        job_id: &str,
        files: &[String],
    ) -> Result<DownloadResponse> {
        let body = FilesBody {
            job_id: job_id.to_string(),
            files: files.to_vec(),
        };
        with_retry("download-links", || async {
            let r = self
                .http
                .post(format!("{BASE}/speech-to-text/job/v1/download-files"))
                .header("api-subscription-key", &self.key)
                .json(&body)
                .send()
                .await?;
            json_ok(r, "download-links").await
        })
        .await
    }

    /// GET the transcript JSON bytes from an Azure presigned URL.
    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>> {
        with_retry("download-get", || async {
            let r = self.http.get(url).send().await?;
            if !r.status().is_success() {
                return Err(anyhow!("download GET failed: {}", r.status()));
            }
            Ok(r.bytes().await?.to_vec())
        })
        .await
    }
}

/// Best-effort content type from a file extension (Azure mostly ignores this).
pub fn content_type_for(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" | "aac" => "audio/aac",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

async fn json_ok<T: serde::de::DeserializeOwned>(resp: reqwest::Response, what: &str) -> Result<T> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("{what} HTTP {status}: {text}"));
    }
    serde_json::from_str::<T>(&text)
        .with_context(|| format!("{what}: decoding response body: {text}"))
}

/// Exponential-backoff retry (5 attempts, base 2s, doubling) — ports the Python `retry`.
async fn with_retry<T, F, Fut>(what: &str, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut delay = BASE_DELAY_SECS;
    for attempt in 1..=MAX_ATTEMPTS {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(e.context(format!("{what} failed after {MAX_ATTEMPTS} attempts")));
                }
                eprintln!(
                    "[retry] {what} attempt {attempt}/{MAX_ATTEMPTS} failed: {e} — retrying in {delay:.0}s"
                );
                tokio::time::sleep(std::time::Duration::from_secs_f64(delay)).await;
                delay *= 2.0;
            }
        }
    }
    unreachable!()
}
