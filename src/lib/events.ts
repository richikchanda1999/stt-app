// Bridge Rust `run://*` events into Svelte stores. All screens are reactive off
// these stores; the DB (via getRunDetail) is the source of truth on (re)hydrate.
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { activeRun, runs, jobProgress } from "./stores";

export async function initEvents(): Promise<() => void> {
  const uns: UnlistenFn[] = [];

  uns.push(
    await listen("run://file-progress", (e) => {
      const p = e.payload as { run_id: string; run_file_id: string; state: string; error: string | null };
      activeRun.update((d) => {
        if (!d || d.run.id !== p.run_id) return d;
        return {
          ...d,
          files: d.files.map((f) =>
            f.id === p.run_file_id
              ? { ...f, state: p.state, error: p.error, has_transcript: p.state === "done" || f.has_transcript }
              : f,
          ),
        };
      });
    }),
  );

  uns.push(
    await listen("run://state", (e) => {
      const p = e.payload as {
        run_id: string;
        aggregate_state: string;
        total: number;
        done: number;
        failed: number;
      };
      runs.update((list) =>
        list.map((r) =>
          r.id === p.run_id
            ? { ...r, aggregate_state: p.aggregate_state, total: p.total, done: p.done, failed: p.failed }
            : r,
        ),
      );
      activeRun.update((d) =>
        d && d.run.id === p.run_id
          ? {
              ...d,
              run: { ...d.run, aggregate_state: p.aggregate_state, total: p.total, done: p.done, failed: p.failed },
            }
          : d,
      );
    }),
  );

  uns.push(
    await listen("run://job-progress", (e) => {
      const p = e.payload as {
        run_id: string;
        job_state: string;
        total: number | null;
        successful: number | null;
        failed: number | null;
      };
      jobProgress.update((m) => ({
        ...m,
        [p.run_id]: { job_state: p.job_state, total: p.total, successful: p.successful, failed: p.failed },
      }));
    }),
  );

  return () => uns.forEach((u) => u());
}
