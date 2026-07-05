<script lang="ts">
  import { api, openFile, revealFile, type RunFile } from "../api";
  import { activeRun, jobProgress, go, showToast } from "../stores";
  import { LANGUAGES, MODELS, MODES, langLabel } from "../langs";
  import StatusBadge from "../components/StatusBadge.svelte";
  import Spinner from "../components/Spinner.svelte";

  let { runId }: { runId: string } = $props();

  let loading = $state(true);
  let expanded = $state<Record<string, any | "loading" | undefined>>({});
  let exporting = $state<Record<string, boolean>>({});
  let showRetry = $state(false);
  let retryLang = $state("unknown");
  let retryModel = $state("saaras:v3");
  let retryMode = $state("transcribe");
  let retrying = $state(false);

  // Reload whenever the target run changes.
  $effect(() => {
    const id = runId;
    loading = true;
    api
      .getRunDetail(id)
      .then((d) => {
        activeRun.set(d);
        retryLang = d.run.default_language;
        retryModel = d.run.model;
        retryMode = d.run.mode ?? "transcribe";
      })
      .catch((e) => showToast(String(e)))
      .finally(() => (loading = false));
  });

  const run = $derived($activeRun && $activeRun.run.id === runId ? $activeRun.run : null);
  const files = $derived($activeRun && $activeRun.run.id === runId ? $activeRun.files : []);
  const prog = $derived($jobProgress[runId]);
  const isRunning = $derived(run?.aggregate_state === "running");
  const failedCount = $derived(files.filter((f) => f.state === "failed").length);

  // Group files by the Sarvam job they belong to (one job per language, <=20 files).
  const fileGroups = $derived.by(() => {
    const map = new Map<string, { key: string; jobId: string | null; language: string; files: typeof files }>();
    for (const f of files) {
      const key = f.job_id ?? "__none__";
      if (!map.has(key)) map.set(key, { key, jobId: f.job_id, language: f.effective_language, files: [] });
      map.get(key)!.files.push(f);
    }
    return [...map.values()];
  });

  async function toggleTranscript(f: RunFile) {
    if (expanded[f.id] !== undefined) {
      const { [f.id]: _, ...rest } = expanded;
      expanded = rest;
      return;
    }
    expanded = { ...expanded, [f.id]: "loading" };
    try {
      const t = await api.getTranscript(f.id);
      expanded = { ...expanded, [f.id]: t };
    } catch (e) {
      showToast(String(e));
      const { [f.id]: _, ...rest } = expanded;
      expanded = rest;
    }
  }

  function fmtTs(sec: number): string {
    const t = Math.round(sec);
    const h = Math.floor(t / 3600);
    const m = Math.floor((t % 3600) / 60);
    const s = t % 60;
    const pad = (n: number) => String(n).padStart(2, "0");
    return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
  }

  async function doExport(f: RunFile) {
    exporting = { ...exporting, [f.id]: true };
    try {
      const path = await api.exportDocx(f.id);
      // reflect docx_path locally
      activeRun.update((d) =>
        d ? { ...d, files: d.files.map((x) => (x.id === f.id ? { ...x, docx_path: path } : x)) } : d,
      );
      await openFile(path);
    } catch (e) {
      showToast(String(e));
    } finally {
      exporting = { ...exporting, [f.id]: false };
    }
  }

  let exportingJob = $state<Record<string, boolean>>({});

  function doneCount(g: { files: typeof files }): number {
    return g.files.filter((f) => f.state === "done").length;
  }

  async function exportJob(g: { key: string; files: typeof files }) {
    const done = g.files.filter((f) => f.state === "done");
    if (done.length === 0) return;
    exportingJob = { ...exportingJob, [g.key]: true };
    let lastPath: string | null = null;
    let failures = 0;
    try {
      for (const f of done) {
        try {
          const path = await api.exportDocx(f.id);
          lastPath = path;
          activeRun.update((d) =>
            d ? { ...d, files: d.files.map((x) => (x.id === f.id ? { ...x, docx_path: path } : x)) } : d,
          );
        } catch {
          failures++;
        }
      }
      const ok = done.length - failures;
      showToast(`Exported ${ok} file${ok === 1 ? "" : "s"} to .docx${failures ? ` (${failures} failed)` : ""}`);
      if (lastPath) await revealFile(lastPath); // open the folder with the docx files
    } finally {
      exportingJob = { ...exportingJob, [g.key]: false };
    }
  }

  async function cancel() {
    try {
      await api.cancelRun(runId);
      showToast("Cancellation requested");
    } catch (e) {
      showToast(String(e));
    }
  }

  async function retry() {
    retrying = true;
    try {
      const newId = await api.retryFailed(runId, {
        language: retryLang,
        model: retryModel,
        mode: retryMode,
      });
      showRetry = false;
      go({ name: "detail", runId: newId });
    } catch (e) {
      showToast(String(e));
    } finally {
      retrying = false;
    }
  }
</script>

<div class="screen">
  <header class="screen-head row-between">
    <div>
      <button class="link" onclick={() => go({ name: "history" })}>← History</button>
      <h1>Run detail</h1>
    </div>
    <div class="head-actions">
      {#if isRunning}
        <button class="btn danger-outline" onclick={cancel}>Cancel run</button>
      {/if}
      {#if failedCount > 0}
        <button class="btn primary" onclick={() => (showRetry = !showRetry)}>
          Retry {failedCount} failed
        </button>
      {/if}
    </div>
  </header>

  {#if loading && !run}
    <p class="muted"><Spinner /> Loading…</p>
  {:else if run}
    <div class="run-summary">
      <StatusBadge state={run.aggregate_state} />
      <span class="run-card-meta">
        {run.model}{run.mode ? ` · ${run.mode}` : ""} · default {langLabel(run.default_language)}
      </span>
      <span class="run-card-counts">
        <span class="ok">{run.done} done</span>
        {#if run.failed > 0}<span class="bad">{run.failed} failed</span>{/if}
        <span class="muted">/ {run.total}</span>
      </span>
      {#if prog && isRunning}
        <span class="muted small">
          job {prog.job_state}{prog.total ? ` · ${prog.successful ?? 0}/${prog.total}` : ""}
        </span>
      {/if}
    </div>

    {#if showRetry}
      <section class="panel retry-panel">
        <h2>Re-run failed files</h2>
        <div class="grid">
          <label class="field">
            <span class="lbl">Language</span>
            <select bind:value={retryLang}>
              {#each LANGUAGES as l}<option value={l.code}>{l.label}</option>{/each}
            </select>
          </label>
          <label class="field">
            <span class="lbl">Model</span>
            <select bind:value={retryModel}>
              {#each MODELS as m}<option value={m.value}>{m.label}</option>{/each}
            </select>
          </label>
          <label class="field" class:disabled={retryModel !== "saaras:v3"}>
            <span class="lbl">Mode</span>
            <select bind:value={retryMode} disabled={retryModel !== "saaras:v3"}>
              {#each MODES as m}<option value={m.value}>{m.label}</option>{/each}
            </select>
          </label>
        </div>
        <div class="toggles">
          <button class="btn primary" onclick={retry} disabled={retrying}>
            {retrying ? "Starting…" : "Start new run with failed files"}
          </button>
          <button class="btn" onclick={() => (showRetry = false)}>Cancel</button>
        </div>
      </section>
    {/if}

    {#each fileGroups as g (g.key)}
      <section class="job-group">
        <div class="job-head">
          <span class="job-label">Job</span>
          {#if g.jobId}
            <code class="job-id" title="Click to select · {g.jobId}">{g.jobId}</code>
          {:else}
            <span class="muted small">not started yet</span>
          {/if}
          <span class="job-meta muted small">{langLabel(g.language)} · {g.files.length} file{g.files.length === 1 ? "" : "s"}</span>
          {#if doneCount(g) > 0}
            <button class="btn sm" onclick={() => exportJob(g)} disabled={exportingJob[g.key]}>
              {exportingJob[g.key] ? "Exporting…" : `Export all (${doneCount(g)})`}
            </button>
          {/if}
        </div>
        <ul class="file-list detail">
          {#each g.files as f (f.id)}
            <li class="file-row detail" class:failed={f.state === "failed"}>
          <div class="file-row-main">
            <div class="file-main">
              <span class="file-name" title={f.original_name}>{f.original_name}</span>
              <span class="file-size">{langLabel(f.effective_language)}</span>
            </div>
            <StatusBadge state={f.state} />
            <div class="row-actions">
              {#if f.state === "done"}
                <button class="btn sm" onclick={() => toggleTranscript(f)}>
                  {expanded[f.id] !== undefined ? "Hide" : "View"}
                </button>
                <button class="btn sm" onclick={() => doExport(f)} disabled={exporting[f.id]}>
                  {exporting[f.id] ? "…" : "Export .docx"}
                </button>
                {#if f.docx_path}
                  <button class="btn sm ghost" onclick={() => f.docx_path && revealFile(f.docx_path)}>Reveal</button>
                {/if}
              {/if}
            </div>
          </div>

          {#if f.error}
            <div class="file-error">{f.error}</div>
          {/if}

          {#if expanded[f.id] === "loading"}
            <div class="transcript"><Spinner /> Loading transcript…</div>
          {:else if expanded[f.id]}
            {@const t = expanded[f.id]}
            <div class="transcript">
              {#if t.diarized_transcript?.entries?.length}
                {#each t.diarized_transcript.entries as e}
                  <p class="tline">
                    <span class="tmeta">[{fmtTs(e.start_time_seconds)}–{fmtTs(e.end_time_seconds)}] Speaker {e.speaker_id}:</span>
                    {e.transcript}
                  </p>
                {/each}
              {:else}
                <p class="tline">{t.transcript}</p>
              {/if}
            </div>
          {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</div>
