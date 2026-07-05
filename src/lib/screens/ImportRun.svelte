<script lang="ts">
  import { onMount } from "svelte";
  import { api, pickAudioFiles, type Asset } from "../api";
  import { LANGUAGES, MODELS, MODES, INHERIT, langLabel } from "../langs";
  import { go, showToast } from "../stores";

  type Row = { asset: Asset; language: string };

  let rows = $state<Row[]>([]);
  let defaultLanguage = $state("unknown");
  let model = $state("saaras:v3");
  let mode = $state("transcribe");
  let numSpeakers = $state<number | null>(null);
  let withDiarization = $state(true);
  let withTimestamps = $state(true);
  let hasKey = $state(true);
  let importing = $state(false);
  let starting = $state(false);

  onMount(async () => {
    try {
      const s = await api.getSettings();
      hasKey = s.has_api_key;
      defaultLanguage = s.default_language;
      model = s.model;
      mode = s.mode ?? "transcribe";
      withDiarization = s.with_diarization;
      withTimestamps = s.with_timestamps;
      numSpeakers = s.num_speakers;
    } catch (e) {
      showToast(String(e));
    }
  });

  const isSaaras = $derived(model === "saaras:v3");

  async function addFiles() {
    importing = true;
    try {
      const paths = await pickAudioFiles();
      if (paths.length === 0) return;
      const assets = await api.importFiles(paths);
      const existing = new Set(rows.map((r) => r.asset.id));
      for (const a of assets) {
        if (!existing.has(a.id)) rows = [...rows, { asset: a, language: INHERIT }];
      }
    } catch (e) {
      showToast(String(e));
    } finally {
      importing = false;
    }
  }

  function removeRow(id: string) {
    rows = rows.filter((r) => r.asset.id !== id);
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  async function run() {
    if (rows.length === 0) return;
    starting = true;
    try {
      const runId = await api.startRun({
        files: rows.map((r) => ({
          asset_id: r.asset.id,
          language: r.language === INHERIT ? null : r.language,
        })),
        default_language: defaultLanguage,
        model,
        mode: isSaaras ? mode : null,
        with_diarization: withDiarization,
        with_timestamps: withTimestamps,
        num_speakers: numSpeakers,
      });
      rows = [];
      go({ name: "detail", runId });
    } catch (e) {
      showToast(String(e));
    } finally {
      starting = false;
    }
  }
</script>

<div class="screen">
  <header class="screen-head">
    <h1>New transcription</h1>
    <p class="sub">Import audio, choose language(s), and run. Everything is stored locally.</p>
  </header>

  {#if !hasKey}
    <div class="banner warn">
      No Sarvam API key set.
      <button class="link" onclick={() => go({ name: "settings" })}>Add it in Settings →</button>
    </div>
  {/if}

  <section class="panel">
    <div class="panel-head">
      <h2>Files</h2>
      <button class="btn" onclick={addFiles} disabled={importing}>
        {importing ? "Importing…" : "＋ Add audio files"}
      </button>
    </div>

    {#if rows.length === 0}
      <button class="dropzone" onclick={addFiles}>
        <span class="dz-icon">🎧</span>
        <span>Click to add audio files</span>
        <span class="dz-hint">mp3 · wav · m4a · flac · ogg · aac · opus · webm</span>
      </button>
    {:else}
      <ul class="file-list">
        {#each rows as r (r.asset.id)}
          <li class="file-row">
            <div class="file-main">
              <span class="file-name" title={r.asset.original_name}>{r.asset.original_name}</span>
              <span class="file-size">{fmtSize(r.asset.size_bytes)}</span>
            </div>
            <label class="lang-inline">
              <span class="lbl">Language</span>
              <select bind:value={r.language}>
                <option value={INHERIT}>Batch default ({langLabel(defaultLanguage)})</option>
                {#each LANGUAGES as l}
                  <option value={l.code}>{l.label}</option>
                {/each}
              </select>
            </label>
            <button class="icon-btn" title="Remove" onclick={() => removeRow(r.asset.id)}>✕</button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="panel">
    <h2>Options</h2>
    <div class="grid">
      <label class="field">
        <span class="lbl">Default language</span>
        <select bind:value={defaultLanguage}>
          {#each LANGUAGES as l}
            <option value={l.code}>{l.label}</option>
          {/each}
        </select>
      </label>

      <label class="field">
        <span class="lbl">Model</span>
        <select bind:value={model}>
          {#each MODELS as m}
            <option value={m.value}>{m.label}</option>
          {/each}
        </select>
      </label>

      <label class="field" class:disabled={!isSaaras}>
        <span class="lbl">Mode {#if !isSaaras}<em>(saaras:v3 only)</em>{/if}</span>
        <select bind:value={mode} disabled={!isSaaras}>
          {#each MODES as m}
            <option value={m.value}>{m.label}</option>
          {/each}
        </select>
      </label>

      <label class="field">
        <span class="lbl">Speakers (optional)</span>
        <input
          type="number"
          min="1"
          placeholder="auto"
          value={numSpeakers ?? ""}
          oninput={(e) => {
            const v = (e.target as HTMLInputElement).value;
            numSpeakers = v === "" ? null : Number(v);
          }}
        />
      </label>
    </div>

    <div class="toggles">
      <label class="check"><input type="checkbox" bind:checked={withDiarization} /> Speaker diarization</label>
      <label class="check"><input type="checkbox" bind:checked={withTimestamps} /> Timestamps</label>
    </div>
  </section>

  <div class="run-bar">
    <span class="run-count">{rows.length} file{rows.length === 1 ? "" : "s"} selected</span>
    <button class="btn primary lg" onclick={run} disabled={rows.length === 0 || starting}>
      {starting ? "Starting…" : "Run transcription"}
    </button>
  </div>
</div>
