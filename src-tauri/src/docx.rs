//! Render a Sarvam transcript JSON into a speaker-labeled, timestamped .docx.
//! Ports `json_to_docx` from the Python CLI.

use anyhow::{Context, Result};
use docx_rs::*;
use std::path::Path;

/// Format seconds as mm:ss (or hh:mm:ss for >= 1h).
fn fmt_timestamp(seconds: f64) -> String {
    let total = seconds.round() as i64;
    let (h, rem) = (total / 3600, total % 3600);
    let (m, s) = (rem / 60, rem % 60);
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub fn json_to_docx(
    json_bytes: &[u8],
    source_name: &str,
    model: &str,
    generated_at: &str,
    out_path: &Path,
) -> Result<()> {
    let data: serde_json::Value =
        serde_json::from_slice(json_bytes).context("parsing transcript JSON")?;

    let mut doc = Docx::new();

    // Heading (bold, ~16pt).
    doc = doc.add_paragraph(
        Paragraph::new().add_run(Run::new().add_text(source_name).bold().size(32)),
    );

    // Italic metadata line.
    let mut meta = format!("Model: {model} | Generated: {generated_at}");
    if let Some(lang) = data.get("language_code").and_then(|v| v.as_str()) {
        meta.push_str(&format!(" | Detected language: {lang}"));
    }
    doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(meta).italic().size(18)));
    doc = doc.add_paragraph(Paragraph::new()); // spacer

    let entries = data
        .get("diarized_transcript")
        .and_then(|d| d.get("entries"))
        .and_then(|e| e.as_array());

    match entries {
        Some(entries) if !entries.is_empty() => {
            for entry in entries {
                let start = entry
                    .get("start_time_seconds")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let end = entry
                    .get("end_time_seconds")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let speaker = entry
                    .get("speaker_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let text = entry.get("transcript").and_then(|v| v.as_str()).unwrap_or("");

                let label = format!(
                    "[{}-{}] Speaker {speaker}: ",
                    fmt_timestamp(start),
                    fmt_timestamp(end)
                );
                doc = doc.add_paragraph(
                    Paragraph::new()
                        .add_run(Run::new().add_text(label).bold())
                        .add_run(Run::new().add_text(text)),
                );
            }
        }
        _ => {
            // Fall back to plain transcript, one paragraph per line.
            let transcript = data
                .get("transcript")
                .and_then(|v| v.as_str())
                .unwrap_or("(empty transcript)");
            for line in transcript.split('\n') {
                doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(line)));
            }
        }
    }

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = std::fs::File::create(out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    doc.build().pack(file).context("packing docx")?;
    Ok(())
}
