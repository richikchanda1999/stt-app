<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { runs, go, showToast } from "../stores";
  import { langLabel } from "../langs";
  import StatusBadge from "../components/StatusBadge.svelte";

  let loading = $state(true);

  onMount(async () => {
    try {
      runs.set(await api.listRuns());
    } catch (e) {
      showToast(String(e));
    } finally {
      loading = false;
    }
  });

  function fmtDate(iso: string): string {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }
</script>

<div class="screen">
  <header class="screen-head">
    <h1>History</h1>
    <p class="sub">Every run is archived here. Open one to view transcripts or retry failures.</p>
  </header>

  {#if loading}
    <p class="muted">Loading…</p>
  {:else if $runs.length === 0}
    <div class="empty">
      <p>No runs yet.</p>
      <button class="btn primary" onclick={() => go({ name: "import" })}>Start a transcription</button>
    </div>
  {:else}
    <ul class="run-list">
      {#each $runs as r (r.id)}
        <li>
          <button class="run-card" onclick={() => go({ name: "detail", runId: r.id })}>
            <div class="run-card-top">
              <StatusBadge state={r.aggregate_state} />
              <span class="run-date">{fmtDate(r.created_at)}</span>
            </div>
            <div class="run-card-meta">
              <span>{r.model}{r.mode ? ` · ${r.mode}` : ""}</span>
              <span>·</span>
              <span>{langLabel(r.default_language)}</span>
              {#if r.parent_run_id}<span class="tag">retry</span>{/if}
            </div>
            <div class="run-card-counts">
              <span class="ok">{r.done} done</span>
              {#if r.failed > 0}<span class="bad">{r.failed} failed</span>{/if}
              <span class="muted">/ {r.total} total</span>
            </div>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
