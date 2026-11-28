<script lang="ts">
  import type { Engine } from '../lib/engine';
  import {
    POINTS,
    curvesFrom,
    effectiveFmaxHz,
    fmaxHz,
    lossAxes,
    readoutText,
    sweep,
    yTop,
    type Curves,
    type UnitSystem,
  } from '../lib/loss';
  import { ticks } from '../lib/plot';
  import Plot, { type Geometry, type Tick, type View } from './Plot.svelte';
  import { FREQ_UNITS, formatG, parseNumber } from '../lib/units';

  interface Props {
    engine: Engine;
    /** False when the result was thrown away by a cancel. */
    canSample: boolean;
    /** Counts results; the view resets when it changes. */
    generation: number;
    /** The mode whose losses are shown, and its impedance with the name of that ("Z0", "Zodd (per trace)"). */
    mode: number;
    impedance: [string, number];
    /** Analysis frequency [Hz]. */
    fHz: number;
    /** Frequency typed in the form [Hz], if it parses. */
    formFHz: number | undefined;
    units: UnitSystem;
    fmaxText: string;
    fmaxUnit: string;
    onFmax: (text: string, unit: string) => void;
  }
  let { engine, canSample, generation, mode, impedance, fHz, formFHz, units, fmaxText, fmaxUnit, onFmax }: Props = $props();

  const floor = $derived(Math.max(fHz, formFHz ?? 0));
  const entryHz = $derived(fmaxHz(fmaxText, fmaxUnit));
  const fmax = $derived(effectiveFmaxHz(entryHz, floor));
  const axes = $derived(lossAxes(units));
  const bad = $derived(parseNumber(fmaxText) === undefined);

  let curves = $state<Curves | undefined>();
  let error = $state('');
  let view = $state<View>([0, 20, 0, 1]); // GHz, GHz, dB/mm, dB/mm
  let hoverX = $state<number | undefined>();
  let readout = $state('');

  // curves for the current result and range
  $effect(() => {
    const [fm, gen, ok, m] = [fmax, generation, canSample, mode];
    void gen;
    if (!ok) {
      curves = undefined;
      return;
    }
    let stale = false;
    const fs = sweep(fm, POINTS);
    engine
      .sample(m, fs)
      .then((data) => {
        if (stale) return;
        curves = curvesFrom(fs, data);
        error = '';
        fullRange();
      })
      .catch((e: unknown) => {
        if (!stale) error = e instanceof Error ? e.message : String(e);
      });
    return () => {
      stale = true;
    };
  });

  // loss at the cursor frequency, evaluated exactly by the module (not read off the curves)
  let seq = 0;
  $effect(() => {
    const x = hoverX;
    if (x === undefined || x <= 0 || !canSample) {
      readout = '';
      return;
    }
    const mine = ++seq;
    engine
      .sample(mode, [x * 1e9])
      .then((row) => {
        if (mine === seq) readout = readoutText(row, x, units);
      })
      .catch(() => {});
  });

  function fullRange() {
    if (curves) view = [0, fmax / 1e9, 0, yTop(curves.total)];
  }

  function fit() {
    if (!curves) return;
    const [x0, x1] = view;
    const inside = curves.total.filter((_, i) => (curves!.f[i] ?? 0) >= x0 && (curves!.f[i] ?? 0) <= x1);
    view = [x0, x1, 0, yTop(inside)];
  }

  const xTicks = (lo: number, hi: number, n: number): Tick[] => ticks(lo, hi, n).map((v) => ({ value: v, label: formatG(v) }));
  // the axis shows the unit system's unit; the data is dB/mm
  const yTicks = (lo: number, hi: number, n: number): Tick[] =>
    ticks(lo * axes.k, hi * axes.k, n).map((v) => ({ value: v / axes.k, label: formatG(v) }));

  const path = (geo: Geometry, ys: number[]): string =>
    curves ? curves.f.map((f, i) => `${i === 0 ? 'M' : 'L'}${geo.sx(f).toFixed(1)},${geo.sy(ys[i] ?? 0).toFixed(1)}`).join('') : '';
</script>

<div class="bar">
  <label>
    f max
    <input type="text" inputmode="decimal" class:invalid={bad} value={fmaxText} oninput={(e) => onFmax(e.currentTarget.value, fmaxUnit)} />
  </label>
  <select aria-label="f max unit" value={fmaxUnit} onchange={(e) => onFmax(fmaxText, e.currentTarget.value)}>
    {#each FREQ_UNITS as [u]}
      <option value={u}>{u}</option>
    {/each}
  </select>
  {#if fmax > entryHz}
    <span class="weak">plot extended past the set frequency ({formatG(floor / 1e9)} GHz)</span>
  {/if}
  <button onclick={fit} disabled={!curves} title="keep the frequency range, scale y to the curves in it">Fit</button>
  <button onclick={fullRange} disabled={!curves} title="0 to f max">Full range</button>
</div>
<p class="caption">Loss vs frequency ({impedance[0]} = {impedance[1].toFixed(2)} Ω; εr, tan δ constant)</p>

{#if !canSample}
  <p class="hint">The last calculation was cancelled, so there is no result to plot. Press Analyze again.</p>
{:else if error}
  <p class="error">{error}</p>
{:else}
  <Plot
    bind:view
    xLabel="Frequency [GHz]"
    yLabel={axes.label}
    {xTicks}
    {yTicks}
    onHover={(x) => (hoverX = x)}
    onReset={fullRange}
    ariaLabel="Conductor, dielectric and total loss versus frequency"
  >
    {#snippet children(geo)}
      {#if curves}
        <path d={path(geo, curves.conductor)} class="line conductor" />
        <path d={path(geo, curves.dielectric)} class="line dielectric" />
        <path d={path(geo, curves.total)} class="line total" />
      {/if}
      <line x1={geo.sx(fHz / 1e9)} x2={geo.sx(fHz / 1e9)} y1={geo.top} y2={geo.top + geo.ih} class="setf" />
      {#if hoverX !== undefined}
        <line x1={geo.sx(hoverX)} x2={geo.sx(hoverX)} y1={geo.top} y2={geo.top + geo.ih} class="cursor" />
      {/if}
    {/snippet}
    {#snippet overlay(geo)}
      <g transform="translate({geo.left + geo.iw - 120} {geo.top + 8})" class="legend">
        <rect width="112" height="58" rx="3" />
        {#each [['Conductor', 'conductor', 1.6], ['Dielectric', 'dielectric', 1.6], ['Total', 'total', 2.4]] as [name, cls, w], i}
          <line x1="8" x2="34" y1={14 + i * 17} y2={14 + i * 17} class="line {cls}" style:stroke-width={w} />
          <text x="42" y={14 + i * 17} dominant-baseline="central">{name}</text>
        {/each}
      </g>
    {/snippet}
  </Plot>
  <p class="readout">{readout || 'Move the cursor over the plot for the losses at that frequency. Wheel zooms, drag pans, double click shows the full range.'}</p>
{/if}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin-bottom: 8px;
  }
  .bar label {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .bar input {
    width: 6rem;
  }
  .weak,
  .hint {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
  .caption {
    margin: 0 0 6px;
  }
  .line {
    fill: none;
    stroke-width: 1.6;
    stroke-linejoin: round;
  }
  .conductor {
    stroke: var(--c-conductor);
  }
  .dielectric {
    stroke: var(--c-dielectric);
  }
  .total {
    stroke: var(--text);
    stroke-width: 2.4;
  }
  .setf {
    stroke: var(--muted);
    stroke-dasharray: 6 4;
    stroke-width: 1;
  }
  .cursor {
    stroke: var(--muted);
    stroke-width: 0.8;
  }
  .legend rect {
    fill: var(--panel);
    stroke: var(--border);
    opacity: 0.9;
  }
  .legend text {
    font-size: 12px;
    fill: var(--text);
  }
  .readout {
    margin: 8px 0 0;
    font-family: var(--mono);
    font-size: 12.5px;
    min-height: 2.8em;
    white-space: pre-wrap;
  }
</style>
