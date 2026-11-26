<script lang="ts">
  import type { SchemColor, Schematic } from '../lib/types';

  let { sketch }: { sketch: Schematic } = $props();

  const MARGIN = 24;
  const fill: Record<SchemColor, string> = {
    UpperDielectric: 'var(--sk-upper)',
    LowerDielectric: 'var(--sk-lower)',
    CustomGap: 'var(--sk-custom)',
    Conductor: 'var(--sk-conductor)',
    ConductorEdge: 'var(--sk-conductor-edge)',
    Signal: 'var(--sk-signal)',
    Text: 'var(--sk-text)',
    Muted: 'var(--sk-muted)',
  };
  const anchor = { Center: 'middle', Left: 'start', Right: 'end' } as const;

  /** Arrow heads at both ends of a line, as polygon point lists. */
  function heads(x0: number, y0: number, x1: number, y1: number, size = 7): string[] {
    const len = Math.hypot(x1 - x0, y1 - y0);
    if (len === 0) return [];
    const [dx, dy] = [(x1 - x0) / len, (y1 - y0) / len];
    const [nx, ny] = [-dy * size * 0.4, dx * size * 0.4];
    const head = (tx: number, ty: number, sx: number, sy: number) => {
      const [bx, by] = [tx + sx * size, ty + sy * size];
      return `${tx},${ty} ${bx + nx},${by + ny} ${bx - nx},${by - ny}`;
    };
    return [head(x0, y0, dx, dy), head(x1, y1, -dx, -dy)];
  }
</script>

<svg
  class="sketch"
  viewBox="{-MARGIN} {-MARGIN} {sketch.width + 2 * MARGIN} {sketch.height + 2 * MARGIN}"
  role="img"
  aria-label="Cross-section of the line (schematic, not to scale)"
>
  <rect x={-MARGIN} y={-MARGIN} width={sketch.width + 2 * MARGIN} height={sketch.height + 2 * MARGIN} fill="var(--sk-bg)" />
  {#each sketch.items as item}
    {#if 'Rect' in item}
      {@const r = item.Rect}
      <rect
        x={Math.min(r.x0, r.x1)}
        y={Math.min(r.y0, r.y1)}
        width={Math.abs(r.x1 - r.x0)}
        height={Math.abs(r.y1 - r.y0)}
        fill={fill[r.fill]}
        stroke={r.outline ? fill[r.outline] : 'none'}
        stroke-width="1"
      />
    {:else if 'Line' in item}
      {@const l = item.Line}
      <line
        x1={l.x0}
        y1={l.y0}
        x2={l.x1}
        y2={l.y1}
        stroke={fill[l.color]}
        stroke-width="1.2"
        stroke-dasharray={l.dashed ? '4 3' : undefined}
      />
      {#if l.arrows}
        {#each heads(l.x0, l.y0, l.x1, l.y1) as pts}
          <polygon points={pts} fill={fill[l.color]} />
        {/each}
      {/if}
    {:else}
      {@const t = item.Text}
      <text x={t.x} y={t.y} text-anchor={anchor[t.anchor]} dominant-baseline="central" font-size="11" font-weight={t.bold ? 700 : 400} fill={fill[t.color]}>
        {t.text}
      </text>
    {/if}
  {/each}
</svg>

<style>
  .sketch {
    display: block;
    width: 100%;
    max-height: 70vh;
    border: 1px solid var(--border);
    border-radius: 8px;
    font-family: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  }
</style>
