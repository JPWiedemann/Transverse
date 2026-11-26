<script lang="ts">
  import { onMount } from 'svelte';
  import ParamForm from './components/ParamForm.svelte';
  import Results from './components/Results.svelte';
  import { Cancelled, Engine } from './lib/engine';
  import * as F from './lib/form';
  import { lineTypes, loadMeta, paramDefaults, paramSpecs, wavelengthOf } from './lib/meta';
  import { applyTheme, loadSettings, saveSettings, type Settings, type Theme } from './lib/settings';
  import type { AnalysisResult, LineInfo, ParamSet, ParamSpec } from './lib/types';
  import { parseNumber } from './lib/units';

  const engine = new Engine();
  const settings: Settings = loadSettings();

  let ready = $state(false);
  let fatal = $state('');
  let line = $state<LineInfo | undefined>();
  let specs = $state<ParamSpec[]>([]);
  let defaults = $state<ParamSet>({});
  let form = $state<F.FormState>({});
  let wavelength: F.Wavelength = () => undefined;
  let synthTarget = $state('');
  let synthZ0 = $state('50');
  let theme = $state<Theme>(settings.theme);

  let busy = $state(false);
  let status = $state('Ready');
  let statusKind = $state<'info' | 'error' | 'ok'>('info');
  let invalidKey = $state<string | null>(null);
  let result = $state<AnalysisResult | undefined>();

  const controls = $derived(F.controlSet(defaults, form));

  onMount(async () => {
    try {
      await loadMeta();
      const first = lineTypes()[0];
      if (!first) throw new Error('no line types in the module');
      selectLine(first);
      ready = true;
    } catch (e) {
      fatal = e instanceof Error ? e.message : String(e);
    }
  });

  $effect(() => {
    applyTheme(theme);
    settings.theme = theme;
    saveSettings(settings);
  });

  function selectLine(l: LineInfo) {
    line = l;
    specs = paramSpecs(l.id);
    defaults = paramDefaults(l.id);
    wavelength = wavelengthOf(l.id);
    let f = F.newForm(specs, defaults, wavelength);
    const saved = settings.lines[l.id];
    f = F.loadSaved(specs, f, saved?.form);
    form = f;
    synthTarget = l.synth_targets.some((t) => t.id === saved?.synthTarget) ? saved!.synthTarget! : (l.synth_targets[0]?.id ?? '');
    synthZ0 = saved?.synthZ0 ?? '50';
  }

  function persist() {
    if (!line) return;
    settings.lines[line.id] = { form: F.toSaved(form), synthTarget, synthZ0 };
    saveSettings(settings);
  }

  function edit(next: F.FormState) {
    form = next;
    invalidKey = null;
    persist();
  }

  const onText = (key: string, text: string) => {
    const cur = form[key];
    if (cur?.kind === 'quantity') edit({ ...form, [key]: { ...cur, text } });
  };
  const onUnit = (key: string, unit: string) => edit(F.changeUnit(specs, defaults, form, key, unit, wavelength));
  const onFlag = (key: string, value: boolean) => edit({ ...form, [key]: { kind: 'flag', value } });
  const onChoice = (key: string, index: number) => edit({ ...form, [key]: { kind: 'choice', index } });

  function setStatus(text: string, kind: 'info' | 'error' | 'ok' = 'info') {
    status = text;
    statusKind = kind;
  }

  function collect(): ParamSet | undefined {
    try {
      return F.toParams(specs, defaults, form, wavelength);
    } catch (e) {
      if (e instanceof F.FormError) invalidKey = e.key;
      setStatus(e instanceof Error ? e.message : String(e), 'error');
      return undefined;
    }
  }

  async function run(start: (p: ParamSet) => Promise<AnalysisResult>, what: string) {
    const params = collect();
    if (!params || !line) return;
    busy = true;
    setStatus(`${what}…`);
    const t0 = performance.now();
    try {
      const r = await start(params);
      result = r;
      if (r.params) form = F.setParams(specs, form, r.params, wavelength);
      persist();
      const z0 = r.summary.modes[0]?.z0;
      setStatus(`${what} done in ${((performance.now() - t0) / 1000).toFixed(1)} s${z0 ? `, Z0 = ${z0.toFixed(3)} Ω` : ''}`, 'ok');
    } catch (e) {
      if (e instanceof Cancelled) setStatus('Cancelled');
      else setStatus(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      busy = false;
    }
  }

  const analyze = () => run((p) => engine.analyze(line!.id, p, (t) => setStatus(t)), 'Analysis');

  function synthesize() {
    const z0 = parseNumber(synthZ0);
    if (z0 === undefined || z0 <= 0) {
      setStatus(`Target impedance: '${synthZ0.trim()}' is not a positive number`, 'error');
      return;
    }
    persist();
    return run((p) => engine.synthesize(line!.id, p, synthTarget, z0, (t) => setStatus(t)), 'Synthesis');
  }
</script>

<svelte:head>
  <title>Transverse{line ? ` – ${line.name}` : ''}</title>
</svelte:head>

{#if fatal}
  <main class="fatal"><h1>Transverse</h1><p>The calculation module could not be loaded: {fatal}</p></main>
{:else if !ready || !line}
  <main class="fatal"><p>Loading…</p></main>
{:else}
  <header>
    <h1>Transverse</h1>
    <span class="line">{line.name}</span>
    <span class="spacer"></span>
    <label class="theme">
      Theme
      <select bind:value={theme}>
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
  </header>

  <div class="actions">
    <button class="primary" disabled={busy} onclick={analyze}>Analyze</button>
    <span class="sep"></span>
    <label>Synthesize
      <select bind:value={synthTarget} disabled={busy} onchange={persist}>
        {#each line.synth_targets as t}
          <option value={t.id}>{t.label}</option>
        {/each}
      </select>
    </label>
    <label>for Z0 =
      <input type="text" inputmode="decimal" class="z0" bind:value={synthZ0} disabled={busy} onchange={persist} /> Ω
    </label>
    <button disabled={busy} onclick={synthesize}>Synthesize</button>
    <span class="sep"></span>
    <button disabled={!busy} onclick={() => engine.cancel()}>Cancel</button>
  </div>

  <div class="layout">
    <section class="form" aria-label="Parameters">
      <ParamForm {specs} {form} {controls} {invalidKey} {onText} {onUnit} {onFlag} {onChoice} />
    </section>
    <section class="output" aria-label="Results">
      <div class="tabs" role="tablist" aria-label="Views">
        <button role="tab" aria-selected="true" class="tab active">Results</button>
      </div>
      {#if result}
        <Results {result} />
      {:else}
        <p class="hint">Enter the geometry and press Analyze.</p>
      {/if}
    </section>
  </div>

  <footer class="status {statusKind}" role="status">
    {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
    {status}
  </footer>
{/if}

<style>
  header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }
  h1 {
    margin: 0;
    font-size: 18px;
  }
  .line {
    color: var(--muted);
  }
  .spacer {
    flex: 1;
  }
  .theme {
    color: var(--muted);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
  }
  .actions label {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .sep {
    width: 1px;
    align-self: stretch;
    background: var(--border);
    margin: 0 4px;
  }
  .z0 {
    width: 5rem;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(22rem, 30rem) 1fr;
    gap: 16px;
    padding: 16px;
    padding-bottom: 56px;
    align-items: start;
  }
  .output {
    min-width: 0;
  }
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
  }
  .tab {
    border-radius: 6px 6px 0 0;
  }
  .tab.active {
    border-bottom: 2px solid var(--accent);
    font-weight: 600;
  }
  .hint {
    color: var(--muted);
  }
  .status {
    position: fixed;
    inset: auto 0 0 0;
    padding: 6px 16px;
    background: var(--panel);
    border-top: 1px solid var(--border);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .status.error {
    color: var(--danger);
  }
  .status.ok {
    color: var(--ok);
  }
  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .fatal {
    padding: 24px;
  }
  @media (max-width: 800px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
</style>
