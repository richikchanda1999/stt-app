<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { LANGUAGES, MODELS, MODES } from "../langs";
  import { go, showToast, settings } from "../stores";

  let apiKey = $state("");
  let hasKey = $state(false);
  let defaultLanguage = $state("unknown");
  let model = $state("saaras:v3");
  let mode = $state("transcribe");
  let withDiarization = $state(true);
  let withTimestamps = $state(true);
  let numSpeakers = $state<number | null>(null);
  let saving = $state(false);

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

  async function save() {
    saving = true;
    try {
      await api.setSettings({
        api_key: apiKey.trim() === "" ? null : apiKey.trim(),
        default_language: defaultLanguage,
        model,
        mode,
        with_diarization: withDiarization,
        with_timestamps: withTimestamps,
        num_speakers: numSpeakers,
      });
      apiKey = "";
      settings.set(await api.getSettings());
      hasKey = ($settings as any)?.has_api_key ?? hasKey;
      showToast("Settings saved");
    } catch (e) {
      showToast(String(e));
    } finally {
      saving = false;
    }
  }
</script>

<div class="screen narrow">
  <header class="screen-head">
    <h1>Settings</h1>
    <p class="sub">Your API key is stored in the OS keychain. All results stay on this machine.</p>
  </header>

  <section class="panel">
    <h2>Sarvam API key</h2>
    <p class="muted small">
      {hasKey ? "✓ A key is currently stored." : "No key stored yet."}
    </p>
    <label class="field">
      <span class="lbl">{hasKey ? "Replace key" : "API key"}</span>
      <input type="password" placeholder="sk_…" bind:value={apiKey} autocomplete="off" />
    </label>
  </section>

  <section class="panel">
    <h2>Defaults for new runs</h2>
    <div class="grid">
      <label class="field">
        <span class="lbl">Default language</span>
        <select bind:value={defaultLanguage}>
          {#each LANGUAGES as l}<option value={l.code}>{l.label}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span class="lbl">Model</span>
        <select bind:value={model}>
          {#each MODELS as m}<option value={m.value}>{m.label}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span class="lbl">Mode</span>
        <select bind:value={mode}>
          {#each MODES as m}<option value={m.value}>{m.label}</option>{/each}
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
    <button class="btn" onclick={() => go({ name: "import" })}>Back</button>
    <button class="btn primary lg" onclick={save} disabled={saving}>
      {saving ? "Saving…" : "Save settings"}
    </button>
  </div>
</div>
