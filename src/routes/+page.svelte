<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import "../app.css";
  import { view, toast, go } from "$lib/stores";
  import { initEvents } from "$lib/events";
  import ImportRun from "$lib/screens/ImportRun.svelte";
  import History from "$lib/screens/History.svelte";
  import RunDetail from "$lib/screens/RunDetail.svelte";
  import Settings from "$lib/screens/Settings.svelte";

  let unlisten: (() => void) | null = null;

  onMount(async () => {
    unlisten = await initEvents();
  });
  onDestroy(() => unlisten?.());
</script>

<div class="app">
  <nav class="topnav">
    <span class="brand">Sarvam STT</span>
    <button class="tab" class:active={$view.name === "import"} onclick={() => go({ name: "import" })}>
      New
    </button>
    <button
      class="tab"
      class:active={$view.name === "history" || $view.name === "detail"}
      onclick={() => go({ name: "history" })}
    >
      History
    </button>
    <button class="tab right" class:active={$view.name === "settings"} onclick={() => go({ name: "settings" })}>
      Settings
    </button>
  </nav>

  <main class="content">
    {#if $view.name === "import"}
      <ImportRun />
    {:else if $view.name === "history"}
      <History />
    {:else if $view.name === "detail"}
      {#key $view.runId}
        <RunDetail runId={$view.runId} />
      {/key}
    {:else if $view.name === "settings"}
      <Settings />
    {/if}
  </main>

  {#if $toast}
    <div class="toast">{$toast}</div>
  {/if}
</div>
