<script lang="ts">
  import { colourbarScale } from '../lib/fieldview';
  import { colormapTable } from '../lib/meta';
  import { ticks } from '../lib/plot';
  import type { Legend } from '../lib/types';
  import { formatG } from '../lib/units';

  let { legend, top, height }: { legend: Legend; top: number; height: number } = $props();

  const BAR = 16;
  let canvas = $state<HTMLCanvasElement | undefined>();
  const scale = $derived(colourbarScale(legend));
  const tickList = $derived(ticks(legend.vmin * scale.mult, legend.vmax * scale.mult, 6));
  const yOf = (t: number) => top + height - ((t / scale.mult - legend.vmin) / (legend.vmax - legend.vmin)) * height;

  $effect(() => {
    if (!canvas) return;
    const table = colormapTable(legend.cmap);
    canvas.width = 1;
    canvas.height = 256;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const img = ctx.createImageData(1, 256);
    for (let i = 0; i < 256; i++) {
      const k = 255 - i; // top = high
      img.data.set([table[k * 3]!, table[k * 3 + 1]!, table[k * 3 + 2]!, 255], i * 4);
    }
    ctx.putImageData(img, 0, 0);
  });
</script>

<div class="bar" style:width="120px" style:height="{top + height + 20}px">
  <canvas bind:this={canvas} style:top="{top}px" style:height="{height}px" style:width="{BAR}px"></canvas>
  <svg width="120" height={top + height + 20} aria-hidden="true">
    {#each tickList as t}
      <line x1={BAR + 6} x2={BAR + 10} y1={yOf(t)} y2={yOf(t)} />
      <text x={BAR + 13} y={yOf(t)} dominant-baseline="central">{formatG(t)}</text>
    {/each}
  </svg>
  <div class="title" style:top="{top + height + 2}px">{scale.title}</div>
</div>

<style>
  .bar {
    position: relative;
    flex: none;
  }
  canvas {
    position: absolute;
    left: 6px;
    border: 1px solid var(--border);
  }
  svg {
    position: absolute;
    inset: 0;
  }
  line {
    stroke: var(--text);
  }
  text {
    font-size: 11px;
    fill: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .title {
    position: absolute;
    left: 6px;
    font-size: 12px;
    white-space: nowrap;
  }
</style>
