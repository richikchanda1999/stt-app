<script lang="ts">
  import Spinner from "./Spinner.svelte";

  let { state }: { state: string } = $props();

  const META: Record<string, { label: string; cls: string; spin: boolean }> = {
    // per-file states
    queued: { label: "Queued", cls: "b-queued", spin: true },
    uploading: { label: "Uploading", cls: "b-active", spin: true },
    processing: { label: "Processing", cls: "b-active", spin: true },
    downloading: { label: "Downloading", cls: "b-active", spin: true },
    done: { label: "Done", cls: "b-done", spin: false },
    failed: { label: "Failed", cls: "b-failed", spin: false },
    cancelled: { label: "Cancelled", cls: "b-cancel", spin: false },
    // run aggregate states
    running: { label: "Running", cls: "b-active", spin: true },
    completed: { label: "Completed", cls: "b-done", spin: false },
    partial: { label: "Partial", cls: "b-partial", spin: false },
  };

  let m = $derived(META[state] ?? { label: state, cls: "b-queued", spin: false });
</script>

<span class="badge {m.cls}">
  {#if m.spin}<Spinner />{/if}
  {m.label}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    line-height: 1.4;
    white-space: nowrap;
  }
  .b-queued {
    background: var(--chip-bg);
    color: var(--muted);
  }
  .b-active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
  .b-done {
    background: color-mix(in srgb, var(--ok) 18%, transparent);
    color: var(--ok);
  }
  .b-failed {
    background: color-mix(in srgb, var(--danger) 16%, transparent);
    color: var(--danger);
  }
  .b-partial {
    background: color-mix(in srgb, var(--warn) 20%, transparent);
    color: var(--warn);
  }
  .b-cancel {
    background: var(--chip-bg);
    color: var(--muted);
  }
</style>
