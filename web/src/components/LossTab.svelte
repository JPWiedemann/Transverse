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
  import { ticks, zoomRange } from '../lib/plot';
  import { FREQ_UNITS, formatG, parseNumber } from '../lib/units';

  interface Props {
    engine: Engine;
    /** False when the result was thrown away by a cancel. */
    canSample: boolean;
    /** Counts results; the view resets when it changes. */
    generation: number;
    z0: number;
    /** Analysis frequency [Hz]. */
    fHz: number;
    /** Frequency typed in the form [Hz], if it parses. */
    formFHz: number | undefined;
    units: UnitSystem;
    fmaxText: string;
    fmaxUnit: string;
    onFmax: (text: string, unit: string) => void;
  }
  let { engine, canSample, generation, z0, fHz, formFHz, units, fmaxText, fmaxUnit, onFmax }: Props = $props();

  const HEIGHT = 380;
  const M = { l: 70, r: 16, t: 14, b: 44 };

  const floor = $derived(Math.max(fHz, formFHz ?? 0));
  const entryHz = $derived(fmaxHz(fmaxText, fmaxUnit));
  const fmax = $derived(effectiveFmaxHz(entryHz, floor));
  const axes = $derived(lossAxes(units));
  const bad = $derived(parseNumber(fmaxText) === undefined);

  let width = $state(700);
  let curves = $state<Curves | undefined>();
  let error = $state('');
  let view = $state<[number, number, number, number]>([0, 20, 0, 1]); // GHz, GHz, dB/mm, dB/mm
  let hoverX = $state<number | undefined>();
  let readout = $state('');

  // curves for the current result and range
  $effect(() => {
    const [fm, gen, ok] = [fmax, generation, canSample];
    void gen;
    if (!ok) {
      curves = undefined;
      return;
    }
    let stale = false;
    const fs = sweep(fm, POINTS);
    engine
      .sample(fs)
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
      .sample([x * 1e9])
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

  const iw = $derived(Math.max(width - M.l - M.r, 10));
  const ih = HEIGHT - M.t - M.b;
  const sx = (x: number) => M.l + ((x - view[0]) / (view[1] - view[0])) * iw;
  const sy = (y: number) => M.t + ih - ((y - view[2]) / (view[3] - view[2])) * ih;
  const dataX = (px: number) => view[0] + ((px - M.l) / iw) * (view[1] - view[0]);
  const dataY = (py: number) => view[2] + ((M.t + ih - py) / ih) * (view[3] - view[2]);

  const xTicks = $derived(ticks(view[0], view[1], Math.max(Math.round(iw / 90), 3)));
  const yTicks = $derived(ticks(view[2] * axes.k, view[3] * axes.k, 6));

  function path(ys: number[]): string {
    if (!curves) return '';
    return curves.f.map((f, i) => `${i === 0 ? 'M' : 'L'}${sx(f).toFixed(1)},${sy(ys[i] ?? 0).toFixed(1)}`).join('');
  }

  function local(e: PointerEvent | WheelEvent, el: Element): [number, number] {
    const r = el.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * width, e.clientY - r.top];
  }

  // wheel zoom needs a non-passive listener to stop the page from scrolling
  function wheel(node: SVGSVGElement) {
    const on = (e: WheelEvent) => {
      e.preventDefault();
      const [px, py] = local(e, node);
      const f = Math.exp(e.deltaY * 0.0015);
      const [x0, x1] = zoomRange(view[0], view[1], dataX(px), f);
      const [y0, y1] = zoomRange(view[2], view[3], dataY(py), f);
      view = [x0, x1, y0, y1];
    };
    node.addEventListener('wheel', on, { passive: false });
    return { destroy: () => node.removeEventListener('wheel', on) };
  }

  let drag: { px: number; py: number; view: [number, number, number, number] } | undefined;

  function down(e: PointerEvent) {
    const el = e.currentTarget as SVGSVGElement;
    const [px, py] = local(e, el);
    drag = { px, py, view: [...view] };
    el.setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    const el = e.currentTarget as SVGSVGElement;
    const [px, py] = local(e, el);
    if (drag) {
      const dx = ((px - drag.px) / iw) * (drag.view[1] - drag.view[0]);
      const dy = ((py - drag.py) / ih) * (drag.view[3] - drag.view[2]);
      view = [drag.view[0] - dx, drag.view[1] - dx, drag.view[2] + dy, drag.view[3] + dy];
    }
    hoverX = px >= M.l && px <= M.l + iw && py >= M.t && py <= M.t + ih ? dataX(px) : undefined;
  }

  function up(e: PointerEvent) {
    drag = undefined;
    (e.currentTarget as SVGSVGElement).releasePointerCapture(e.pointerId);
  }
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
<p class="caption">Loss vs frequency (Z0 = {z0.toFixed(2)} Ω; εr, tan δ constant)</p>

{#if !canSample}
  <p class="hint">The last calculation was cancelled, so there is no result to plot. Press Analyze again.</p>
{:else if error}
  <p class="error">{error}</p>
{:else}
  <div class="plot" bind:clientWidth={width}>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <svg
      {width}
      height={HEIGHT}
      role="img"
      aria-label="Conductor, dielectric and total loss versus frequency"
      use:wheel
      onpointerdown={down}
      onpointermove={move}
      onpointerup={up}
      onpointerleave={() => (hoverX = undefined)}
      ondblclick={fullRange}
    >
      <defs>
        <clipPath id="loss-clip"><rect x={M.l} y={M.t} width={iw} height={ih} /></clipPath>
      </defs>
      <rect x={M.l} y={M.t} width={iw} height={ih} class="frame" />
      {#each xTicks as t}
        <line x1={sx(t)} x2={sx(t)} y1={M.t} y2={M.t + ih} class="grid" />
        <text x={sx(t)} y={M.t + ih + 16} text-anchor="middle" class="tick">{formatG(t)}</text>
      {/each}
      {#each yTicks as t}
        <line x1={M.l} x2={M.l + iw} y1={sy(t / axes.k)} y2={sy(t / axes.k)} class="grid" />
        <text x={M.l - 6} y={sy(t / axes.k)} text-anchor="end" dominant-baseline="central" class="tick">{formatG(t)}</text>
      {/each}
      <text x={M.l + iw / 2} y={HEIGHT - 6} text-anchor="middle" class="axis">Frequency [GHz]</text>
      <text transform="translate(14 {M.t + ih / 2}) rotate(-90)" text-anchor="middle" class="axis">{axes.label}</text>

      <g clip-path="url(#loss-clip)">
        {#if curves}
          <path d={path(curves.conductor)} class="line conductor" />
          <path d={path(curves.dielectric)} class="line dielectric" />
          <path d={path(curves.total)} class="line total" />
        {/if}
        <line x1={sx(fHz / 1e9)} x2={sx(fHz / 1e9)} y1={M.t} y2={M.t + ih} class="setf" />
        {#if hoverX !== undefined}
          <line x1={sx(hoverX)} x2={sx(hoverX)} y1={M.t} y2={M.t + ih} class="cursor" />
        {/if}
      </g>

      <g transform="translate({M.l + iw - 120} {M.t + 8})" class="legend">
        <rect width="112" height="58" rx="3" />
        {#each [['Conductor', 'conductor', 1.6], ['Dielectric', 'dielectric', 1.6], ['Total', 'total', 2.4]] as [name, cls, w], i}
          <line x1="8" x2="34" y1={14 + i * 17} y2={14 + i * 17} class="line {cls}" style:stroke-width={w} />
          <text x="42" y={14 + i * 17} dominant-baseline="central">{name}</text>
        {/each}
      </g>
    </svg>
  </div>
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
  .plot {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  svg {
    display: block;
    touch-action: none;
    cursor: crosshair;
    user-select: none;
  }
  .frame {
    fill: none;
    stroke: var(--border);
  }
  .grid {
    stroke: var(--border);
    stroke-width: 0.6;
    opacity: 0.7;
  }
  .tick {
    font-size: 11px;
    fill: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .axis {
    font-size: 12px;
    fill: var(--text);
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
