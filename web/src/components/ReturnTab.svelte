<script lang="ts">
  import { ticks } from '../lib/plot';
  import { allBounds, fitBounds, readoutText, segments, yFit } from '../lib/returncurrent';
  import type { SplitSummary } from '../lib/types';
  import { autoLengthUnit, formatG, type UnitSystem } from '../lib/units';
  import Plot, { type Geometry, type Tick, type View } from './Plot.svelte';

  let { split, generation, units }: { split: SplitSummary; generation: number; units: UnitSystem } = $props();

  const CYCLE = ['var(--c-conductor)', 'var(--c-dielectric)', 'var(--c-third)'];
  const colour = (i: number) => CYCLE[i % CYCLE.length]!;

  let view = $state<View>([0, 1, 0, 1]); // set by the effect below
  let hoverX = $state<number | undefined>();

  // a new result starts at the signal region
  $effect(() => {
    void generation;
    view = fitBounds(split);
  });

  // x in a length unit that suits the visible range, J_s/I per that unit
  const unit = $derived(autoLengthUnit(units, view[1] - view[0]));
  const uf = $derived(unit[1]);
  const ymax = $derived(fitBounds(split)[3]);

  const xTicks = (lo: number, hi: number, n: number): Tick[] =>
    ticks(lo / uf, hi / uf, n).map((v) => ({ value: v * uf, label: formatG(v) }));
  const yTicks = (lo: number, hi: number, n: number): Tick[] =>
    ticks(lo * uf, hi * uf, n).map((v) => ({ value: v / uf, label: formatG(v) }));

  const path = (geo: Geometry, seg: [number, number][]) =>
    seg.map(([x, j], i) => `${i === 0 ? 'M' : 'L'}${geo.sx(x).toFixed(1)},${geo.sy(j).toFixed(1)}`).join('');

  const readout = $derived(hoverX === undefined ? '' : readoutText(split, hoverX, unit[0], uf));
</script>

<table class="split">
  <thead>
    <tr><th>Ground conductor</th><th>E-field share</th><th>Return current (HF)</th></tr>
  </thead>
  <tbody>
    {#each split.rows as r (r.key)}
      <tr>
        <td style:padding-left="{r.indent * 18 + 8}px">{r.label}</td>
        <td class="num">{r.field_pct.toFixed(1)} %</td>
        <td class="num">{r.current_pct.toFixed(1)} %</td>
      </tr>
    {/each}
  </tbody>
</table>
<p class="info">{split.info}</p>

<div class="bar">
  <button onclick={() => (view = [view[0], view[1], 0, yFit(split, view[0], view[1])])} title="keep the x range, scale y to the curves in it">Fit y</button>
  <button onclick={() => (view = fitBounds(split))} title="the trace and its gaps plus the nearest ground">Signal region</button>
  <button onclick={() => (view = allBounds(split))} title="the whole domain">Full view</button>
  <span class="weak">wheel: zoom   drag: pan   double-click: reset</span>
</div>

<Plot
  bind:view
  xLabel="x [{unit[0]}]"
  yLabel="J_s / I  [1/{unit[0]}]"
  {xTicks}
  {yTicks}
  onHover={(x) => (hoverX = x)}
  onReset={() => (view = fitBounds(split))}
  ariaLabel="Return current density along the ground conductors"
>
  {#snippet children(geo)}
    {#if split.plot.signal}
      {@const [a, b] = split.plot.signal}
      <rect x={geo.sx(a)} y={geo.sy(ymax)} width={Math.max(geo.sx(b) - geo.sx(a), 0)} height={Math.max(geo.sy(0) - geo.sy(ymax), 0)} class="signal" />
    {/if}
    {#each split.plot.markers as x}
      <line x1={geo.sx(x)} x2={geo.sx(x)} y1={geo.top} y2={geo.top + geo.ih} class="marker" />
    {/each}
    {#each split.profiles as p (p.key)}
      {#each segments(p) as seg}
        {#if seg.length > 0}
          <path d={path(geo, seg)} class="line" class:dashed={p.dashed} style:stroke={colour(p.color)} />
        {/if}
      {/each}
    {/each}
    {#each split.plot.labels as [x, text]}
      <text x={geo.sx(x)} y={geo.sy(ymax * 0.97) + 4} text-anchor="middle" dominant-baseline="hanging" class="label">{text}</text>
    {/each}
    {#if hoverX !== undefined}
      <line x1={geo.sx(hoverX)} x2={geo.sx(hoverX)} y1={geo.top} y2={geo.top + geo.ih} class="cursor" />
    {/if}
  {/snippet}
  {#snippet overlay(geo)}
    {@const h = split.profiles.length * 17 + 10}
    {@const w = 24 + 8 * Math.max(...split.profiles.map((p) => p.label.length + 7), 8)}
    <g transform="translate({geo.left + geo.iw - w - 8} {geo.top + 22})" class="legend">
      <rect width={w} height={h} rx="3" />
      {#each split.profiles as p, i (p.key)}
        <line x1="8" x2="34" y1={14 + i * 17} y2={14 + i * 17} class="line" class:dashed={p.dashed} style:stroke={colour(p.color)} />
        <text x="42" y={14 + i * 17} dominant-baseline="central">{p.label}, {p.share_pct.toFixed(1)} %</text>
      {/each}
    </g>
  {/snippet}
</Plot>
<p class="readout">{readout || 'Move the cursor over the plot for the current density of every curve at that x.'}</p>

<style>
  .split {
    align-self: flex-start;
    border-collapse: collapse;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 6px;
  }
  th,
  td {
    padding: 3px 12px 3px 8px;
    text-align: left;
  }
  th {
    color: var(--muted);
    font-weight: 600;
    border-bottom: 1px solid var(--border);
  }
  tbody tr:nth-child(even) {
    background: color-mix(in srgb, var(--border) 25%, transparent);
  }
  .num {
    font-family: var(--mono);
    text-align: right;
    white-space: nowrap;
  }
  .info {
    margin: 4px 0 8px;
    color: var(--muted);
    white-space: pre-wrap;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    margin-bottom: 8px;
  }
  .weak {
    color: var(--muted);
  }
  .line {
    fill: none;
    stroke-width: 1.4;
    stroke-linejoin: round;
  }
  .dashed {
    stroke-dasharray: 6 4;
  }
  .signal {
    fill: var(--c-conductor);
    opacity: 0.1;
  }
  .marker {
    stroke: var(--muted);
    stroke-dasharray: 2 4;
    stroke-width: 0.8;
  }
  .cursor {
    stroke: var(--muted);
    stroke-width: 0.8;
  }
  .label {
    font-size: 11px;
    fill: var(--text);
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
