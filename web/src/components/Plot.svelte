<script lang="ts" module>
  export type View = [number, number, number, number]; // x0, x1, y0, y1 in data units
  export interface Tick {
    /** position in data units */
    value: number;
    label: string;
  }
  export interface Geometry {
    sx: (x: number) => number;
    sy: (y: number) => number;
    iw: number;
    ih: number;
    left: number;
    top: number;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { zoomRange } from '../lib/plot';

  // A line plot in SVG: axes, grid, wheel zoom around the cursor, drag to pan, double click to
  // reset. The curves are drawn by the caller (`children`, clipped to the plot area).
  interface Props {
    view: View;
    xLabel: string;
    yLabel: string;
    /** Ticks for the visible range; `approx` is a suggested count. */
    xTicks: (lo: number, hi: number, approx: number) => Tick[];
    yTicks: (lo: number, hi: number, approx: number) => Tick[];
    height?: number;
    /** Reports the x value [data units] under the cursor, `undefined` outside the plot. */
    onHover?: (x: number | undefined) => void;
    onReset?: () => void;
    ariaLabel: string;
    children: Snippet<[Geometry]>;
    /** Drawn above the curves, not clipped (legend). */
    overlay?: Snippet<[Geometry]>;
  }
  let { view = $bindable(), xLabel, yLabel, xTicks, yTicks, height = 380, onHover, onReset, ariaLabel, children, overlay }: Props = $props();

  const M = { l: 70, r: 16, t: 14, b: 44 };
  const clipId = `clip-${Math.random().toString(36).slice(2)}`;

  let width = $state(700);
  const iw = $derived(Math.max(width - M.l - M.r, 10));
  const ih = $derived(height - M.t - M.b);
  const sx = (x: number) => M.l + ((x - view[0]) / (view[1] - view[0])) * iw;
  const sy = (y: number) => M.t + ih - ((y - view[2]) / (view[3] - view[2])) * ih;
  const dataX = (px: number) => view[0] + ((px - M.l) / iw) * (view[1] - view[0]);
  const dataY = (py: number) => view[2] + ((M.t + ih - py) / ih) * (view[3] - view[2]);
  const geo = $derived<Geometry>({ sx, sy, iw, ih, left: M.l, top: M.t });

  const xt = $derived(xTicks(view[0], view[1], Math.max(Math.round(iw / 90), 3)));
  const yt = $derived(yTicks(view[2], view[3], 6));

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

  let drag: { px: number; py: number; view: View } | undefined;

  function down(e: PointerEvent) {
    const el = e.currentTarget as SVGSVGElement;
    const [px, py] = local(e, el);
    drag = { px, py, view: [...view] };
    el.setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    const [px, py] = local(e, e.currentTarget as SVGSVGElement);
    if (drag) {
      const dx = ((px - drag.px) / iw) * (drag.view[1] - drag.view[0]);
      const dy = ((py - drag.py) / ih) * (drag.view[3] - drag.view[2]);
      view = [drag.view[0] - dx, drag.view[1] - dx, drag.view[2] + dy, drag.view[3] + dy];
    }
    onHover?.(px >= M.l && px <= M.l + iw && py >= M.t && py <= M.t + ih ? dataX(px) : undefined);
  }

  function up(e: PointerEvent) {
    drag = undefined;
    (e.currentTarget as SVGSVGElement).releasePointerCapture(e.pointerId);
  }
</script>

<div class="plot" bind:clientWidth={width}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <svg
    {width}
    {height}
    role="img"
    aria-label={ariaLabel}
    use:wheel
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointerleave={() => onHover?.(undefined)}
    ondblclick={onReset}
  >
    <defs>
      <clipPath id={clipId}><rect x={M.l} y={M.t} width={iw} height={ih} /></clipPath>
    </defs>
    <rect x={M.l} y={M.t} width={iw} height={ih} class="frame" />
    {#each xt as t}
      <line x1={sx(t.value)} x2={sx(t.value)} y1={M.t} y2={M.t + ih} class="grid" />
      <text x={sx(t.value)} y={M.t + ih + 16} text-anchor="middle" class="tick">{t.label}</text>
    {/each}
    {#each yt as t}
      <line x1={M.l} x2={M.l + iw} y1={sy(t.value)} y2={sy(t.value)} class="grid" />
      <text x={M.l - 6} y={sy(t.value)} text-anchor="end" dominant-baseline="central" class="tick">{t.label}</text>
    {/each}
    <text x={M.l + iw / 2} y={height - 6} text-anchor="middle" class="axis">{xLabel}</text>
    <text transform="translate(14 {M.t + ih / 2}) rotate(-90)" text-anchor="middle" class="axis">{yLabel}</text>
    <g clip-path="url(#{clipId})">{@render children(geo)}</g>
    {@render overlay?.(geo)}
  </svg>
</div>

<style>
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
</style>
