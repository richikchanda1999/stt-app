// Typed wrappers around the Rust commands + Tauri plugins.
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

export type Asset = {
  id: string;
  sha256: string;
  original_name: string;
  ext: string;
  size_bytes: number;
  stored_path: string;
  imported_at: string;
};

export type RunFile = {
  id: string;
  asset_id: string;
  original_name: string;
  job_id: string | null;
  effective_language: string;
  state: string;
  error: string | null;
  has_transcript: boolean;
  docx_path: string | null;
};

export type RunSummary = {
  id: string;
  created_at: string;
  model: string;
  mode: string | null;
  default_language: string;
  with_diarization: boolean;
  with_timestamps: boolean;
  num_speakers: number | null;
  aggregate_state: string;
  parent_run_id: string | null;
  total: number;
  done: number;
  failed: number;
};

export type RunDetail = { run: RunSummary; files: RunFile[] };

export type Settings = {
  has_api_key: boolean;
  default_language: string;
  model: string;
  mode: string | null;
  with_diarization: boolean;
  with_timestamps: boolean;
  num_speakers: number | null;
};

export type StartRunInput = {
  files: { asset_id: string; language: string | null }[];
  default_language: string;
  model: string;
  mode: string | null;
  with_diarization: boolean;
  with_timestamps: boolean;
  num_speakers: number | null;
};

export type RetryInput = {
  language?: string | null;
  model?: string | null;
  mode?: string | null;
  with_diarization?: boolean | null;
  with_timestamps?: boolean | null;
  num_speakers?: number | null;
};

export type UpdateInfo = { version: string; current: string; url: string };

export const api = {
  checkForUpdate: () => invoke<UpdateInfo | null>("check_for_update"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (input: Omit<Settings, "has_api_key"> & { api_key?: string | null }) =>
    invoke<void>("set_settings", { input }),
  importFiles: (paths: string[]) => invoke<Asset[]>("import_files", { paths }),
  listRuns: () => invoke<RunSummary[]>("list_runs"),
  getRunDetail: (runId: string) => invoke<RunDetail>("get_run_detail", { runId }),
  getTranscript: (runFileId: string) => invoke<any>("get_transcript", { runFileId }),
  startRun: (input: StartRunInput) => invoke<string>("start_run", { input }),
  retryFailed: (runId: string, overrides: RetryInput) =>
    invoke<string>("retry_failed", { runId, overrides }),
  cancelRun: (runId: string) => invoke<void>("cancel_run", { runId }),
  exportDocx: (runFileId: string) => invoke<string>("export_docx", { runFileId }),
};

const AUDIO_EXTS = ["mp3", "wav", "m4a", "flac", "ogg", "aac", "opus", "webm"];

export async function pickAudioFiles(): Promise<string[]> {
  const res = await openDialog({
    multiple: true,
    directory: false,
    filters: [{ name: "Audio", extensions: AUDIO_EXTS }],
  });
  if (!res) return [];
  return Array.isArray(res) ? res : [res];
}

export async function openFile(path: string) {
  await openPath(path);
}

export async function revealFile(path: string) {
  await revealItemInDir(path);
}

export async function openInBrowser(url: string) {
  await openUrl(url);
}
