import { writable } from "svelte/store";
import type { RunSummary, RunDetail, Settings, UpdateInfo } from "./api";

export type View =
  | { name: "import" }
  | { name: "history" }
  | { name: "detail"; runId: string }
  | { name: "settings" };

export const view = writable<View>({ name: "import" });
export const runs = writable<RunSummary[]>([]);
export const activeRun = writable<RunDetail | null>(null);
export const settings = writable<Settings | null>(null);

export type JobProg = {
  job_state: string;
  total: number | null;
  successful: number | null;
  failed: number | null;
};
export const jobProgress = writable<Record<string, JobProg>>({});

export const update = writable<UpdateInfo | null>(null);

export const toast = writable<string | null>(null);
let toastTimer: ReturnType<typeof setTimeout> | null = null;
export function showToast(msg: string) {
  toast.set(msg);
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.set(null), 4000);
}

export function go(v: View) {
  view.set(v);
}
