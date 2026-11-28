<script lang="ts">
  import { onMount } from 'svelte';
  import CrossSection from './components/CrossSection.svelte';
  import FieldTab from './components/FieldTab.svelte';
  import LossTab from './components/LossTab.svelte';
  import ParamForm from './components/ParamForm.svelte';
  import ReturnTab from './components/ReturnTab.svelte';
  import Results from './components/Results.svelte';
  import { Cancelled, Engine } from './lib/engine';
  import * as F from './lib/form';
  import { lineTypes, loadMeta, paramDefaults, paramSpecs, schematicOf, wavelengthOf } from './lib/meta';
  import { applyTheme, loadSettings, saveSettings, type FieldMode, type Settings, type Theme } from './lib/settings';
  import type { AnalysisResult, LineInfo, ParamSet, ParamSpec } from './lib/types';
  import { parseNumber, type UnitSystem } from './lib/units';

  type Tab = 'cross-section' | 'results' | 'field' | 'return-current' | 'loss';

  const engine = new Engine();
  const tabs: [Tab, string][] = [
    ['cross-section', 'Cross-section'],
    ['results', 'Results'],
    ['field', 'Field'],
    ['return-current', 'Return current'],
    ['loss', 'Loss vs f'],
  ];
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
  // electrical length to reach in degrees; empty keeps the line length
  let synthPhase = $state('');
  let theme = $state<Theme>(settings.theme);
  let units = $state<UnitSystem>(settings.units);
  let tab = $state<Tab>('cross-section');
  let lossFmaxText = $state(settings.lossFmaxText);
  let lossFmaxUnit = $state(settings.lossFmaxUnit);
  let fieldMode = $state<FieldMode>(settings.fieldMode);
  let showEq = $state(settings.showEq);
  let showMesh = $state(settings.showMesh);
  let dbRange = $state(settings.dbRange);
  // counts results (the loss plot resets its view); false after a cancel threw the result away
  let generation = $state(0);
  let canSample = $state(false);

  let busy = $state(false);
  let status = $state('Ready');
  let statusKind = $state<'info' | 'error' | 'ok'>('info');
  let invalidKey = $state<string | null>(null);
  let result = $state<AnalysisResult | undefined>();

  const controls = $derived(F.controlSet(defaults, form));
  // the synthesis targets the structure offers, and the chosen one among them (else the first)
  const targets = $derived(line ? line.synth_targets.filter((t) => F.isVisible(t.visible_when, controls)) : []);
  const target = $derived(targets.some((t) => t.id === synthTarget) ? synthTarget : (targets[0]?.id ?? ''));

  // the parameters as currently typed, if they are valid (sketch and loss plot follow the form)
  const current = $derived.by(() => {
    if (!line) return undefined;
    try {
      return F.toParams(specs, defaults, form, wavelength);
    } catch {
      return undefined;
    }
  });
  const sketch = $derived.by(() => {
    if (!line || !current) return undefined;
    try {
      return schematicOf(line.id, current);
    } catch {
      return undefined;
    }
  });
  const formFHz = $derived(typeof current?.f === 'number' ? current.f : undefined);

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
    settings.units = units;
    settings.lossFmaxText = lossFmaxText;
    settings.lossFmaxUnit = lossFmaxUnit;
    settings.fieldMode = fieldMode;
    settings.showEq = showEq;
    settings.showMesh = showMesh;
    settings.dbRange = Number.isFinite(dbRange) ? dbRange : 40;
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
    synthPhase = saved?.synthPhase ?? '';
  }

  function persist() {
    if (!line) return;
    settings.lines[line.id] = { form: F.toSaved(form), synthTarget, synthZ0, synthPhase };
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
      generation += 1;
      if (tab === 'cross-section') tab = 'results';
      canSample = engine.canSample;
      if (r.params) form = F.setParams(specs, form, r.params, wavelength);
      persist();
      const z0 = r.summary.modes[0]?.z0;
      setStatus(`${what} done in ${((performance.now() - t0) / 1000).toFixed(1)} s${z0 ? `, Z0 = ${z0.toFixed(3)} Ω` : ''}`, 'ok');
    } catch (e) {
      if (e instanceof Cancelled) setStatus('Cancelled');
      else setStatus(e instanceof Error ? e.message : String(e), 'error');
    } finally {
      canSample = engine.canSample;
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
    // the phase is optional: the line length follows from it (and eps_eff) after the Z0 synthesis
    let phase: number | undefined;
    if (synthPhase.trim() !== '') {
      phase = parseNumber(synthPhase);
      if (phase === undefined || phase <= 0) {
        setStatus(`Target phase: '${synthPhase.trim()}' is not a positive number of degrees (leave it empty to keep the length)`, 'error');
        return;
      }
    }
    persist();
    return run((p) => engine.synthesize(line!.id, p, target, z0, phase, (t) => setStatus(t)), 'Synthesis');
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
  <div class="app">
  <header>
    <h1>Transverse</h1>
    <span class="line">{line.name}</span>
    <span class="spacer"></span>
    <label class="theme">
      Units
      <select bind:value={units}>
        <option value="metric">Metric</option>
        <option value="imperial">Imperial</option>
      </select>
    </label>
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
      <select value={target} disabled={busy} onchange={(e) => { synthTarget = e.currentTarget.value; persist(); }}>
        {#each targets as t (t.id)}
          <option value={t.id}>{t.label}</option>
        {/each}
      </select>
    </label>
    <label>for Z0 =
      <input type="text" inputmode="decimal" class="value" aria-label="Target Z0" bind:value={synthZ0} disabled={busy} onchange={persist} /> Ω
    </label>
    <label title="electrical length at the analysis frequency; the line length is set from it. Empty keeps the line length.">phase =
      <input type="text" inputmode="decimal" class="value" aria-label="Target phase" placeholder="keep length" bind:value={synthPhase} disabled={busy} onchange={persist} /> °
    </label>
    <button disabled={busy} onclick={synthesize}>Synthesize</button>
    <span class="sep"></span>
    <button disabled={!busy} onclick={() => engine.cancel()}>Cancel</button>
  </div>

  <div class="layout">
    <section class="form" aria-label="Parameters">
      <ParamForm {specs} {form} {controls} presets={line.presets} onPreset={(p) => edit(F.applyPreset(form, p))} {invalidKey} {onText} {onUnit} {onFlag} {onChoice} />
    </section>
    <section class="output" aria-label="Results">
      <div class="tabs" role="tablist" aria-label="Views">
        {#each tabs as [id, label]}
          <button role="tab" aria-selected={tab === id} class="tab" class:active={tab === id} onclick={() => (tab = id)}>{label}</button>
        {/each}
      </div>
      <div class="tabbody">
      {#if tab === 'results'}
        {#if result}
          <Results {result} />
        {:else}
          <p class="hint">Enter the geometry and press Analyze.</p>
        {/if}
      {:else if tab === 'cross-section'}
        {#if sketch}
          <CrossSection {sketch} />
        {:else}
          <p class="hint">{current ? 'This line type has no cross-section sketch.' : 'Fix the invalid parameter to see the sketch.'}</p>
        {/if}
      {:else if tab === 'return-current'}
        {@const split = result?.summary.modes[0]?.split}
        {#if split}
          <ReturnTab {split} {generation} {units} />
        {:else}
          <p class="hint">Press Analyze to see how the return current splits over the ground conductors.</p>
        {/if}
      {:else if tab === 'field'}
        {#if result?.summary.has_field}
          <FieldTab
            {engine}
            {canSample}
            {generation}
            {units}
            accuracy={result.summary.accuracy}
            bind:mode={fieldMode}
            bind:showEq
            bind:showMesh
            bind:dbRange
          />
        {:else}
          <p class="hint">Press Analyze to see the field.</p>
        {/if}
      {:else if result}
        {@const mode = result.summary.modes[0]}
        {#if mode}
          <LossTab
            {engine}
            {canSample}
            {generation}
            z0={mode.z0}
            fHz={mode.f_hz}
            {formFHz}
            {units}
            fmaxText={lossFmaxText}
            fmaxUnit={lossFmaxUnit}
            onFmax={(t, u) => {
              lossFmaxText = t;
              lossFmaxUnit = u;
            }}
          />
        {/if}
      {:else}
        <p class="hint">Press Analyze to see the loss over frequency.</p>
      {/if}
      </div>
    </section>
  </div>

  <footer class="status {statusKind}" role="status">
    {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
    {status}
  </footer>
  </div>
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
  .value {
    width: 5rem;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100dvh;
  }
  .layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(22rem, 30rem) 1fr;
    gap: 16px;
    padding: 16px;
  }
  .form {
    overflow-y: auto;
    min-height: 0;
    padding-right: 4px;
  }
  .output {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  /* the plots take the height that is left */
  .tabbody {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
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
    flex: none;
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
    .app {
      height: auto;
      min-height: 100dvh;
    }
    .layout {
      grid-template-columns: 1fr;
    }
    .form {
      overflow: visible;
    }
  }
</style>
