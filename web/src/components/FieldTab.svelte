<script lang="ts">
  import type { Engine } from '../lib/engine';
  import {
    MARGIN,
    boundsOf,
    contains,
    expand,
    fitView,
    needsRender,
    panBy,
    zoomAt,
    type Bounds,
    type Rendered,
    type View,
  } from '../lib/fieldview';
  import type { RasterResult } from '../lib/protocol';
  import { ticks } from '../lib/plot';
  import { FIELD_MODES, type FieldMode } from '../lib/settings';
  import type { Accuracy, FieldInfo, Legend, Probe } from '../lib/types';
  import {
    autoLengthUnit,
    fieldDbUnit,
    fieldPerVPerM,
    formatField,
    formatG,
    formatSig,
    type UnitSystem,
  } from '../lib/units';
  import ColorBar from './ColorBar.svelte';

  interface Props {
    engine: Engine;
    canSample: boolean;
    generation: number;
    units: UnitSystem;
    accuracy: Accuracy | null;
    mode: FieldMode;
    showEq: boolean;
    showMesh: boolean;
    dbRange: number;
  }
  let {
    engine,
    canSample,
    generation,
    units,
    accuracy,
    mode = $bindable(),
    showEq = $bindable(),
    showMesh = $bindable(),
    dbRange = $bindable(),
  }: Props = $props();

  const LABELS: Record<FieldMode, string> = {
    e_db_lines: '|E| [dB] + field lines',
    e_db: '|E| [dB]',
    e_linear: '|E| (linear)',
    potential: 'Potential',
    permittivity: 'Permittivity er',
    mesh: 'Mesh',
  };
  const M = { l: 70, t: 8, r: 8, b: 44 };
  const COPPER = '#c87533';

  const isDb = $derived(mode === 'e_db' || mode === 'e_db_lines');
  const hasLines = $derived(mode === 'e_db_lines');
  const allowsEq = $derived(mode !== 'permittivity' && mode !== 'mesh');
  const meshOn = $derived(showMesh || mode === 'mesh');

  // ---- result data
  let info = $state<FieldInfo | undefined>();
  let gx = $state<Float64Array>(new Float64Array());
  let gy = $state<Float64Array>(new Float64Array());
  let eqPath = $state('');
  let error = $state('');

  // ---- view
  let boxW = $state(700);
  const W = $derived(Math.max(boxW, 300));
  let boxH = $state(420);
  const H = $derived(Math.max(boxH, 300));
  const PW = $derived(W - M.l - M.r);
  const PH = $derived(H - M.t - M.b);
  let view = $state<View>({ cx: 0, cy: 0, ppm: 1 });
  const bounds = $derived(boundsOf(view, PW, PH));
  const lengthUnit = $derived(autoLengthUnit(units, Math.max(bounds[1] - bounds[0], bounds[3] - bounds[2])));
  const uf = $derived(lengthUnit[1]);

  const px = (x: number) => PW / 2 + (x - view.cx) * view.ppm;
  const py = (y: number) => PH / 2 - (y - view.cy) * view.ppm;
  const toX = (p: number) => view.cx + (p - M.l - PW / 2) / view.ppm;
  const toY = (p: number) => view.cy - (p - M.t - PH / 2) / view.ppm;

  function showRegion(b: Bounds) {
    view = fitView(b, PW, PH);
  }

  // ---- load a new result
  $effect(() => {
    void generation;
    if (!canSample) {
      info = undefined;
      return;
    }
    let stale = false;
    // the field data says which equipotentials suit its excitation
    engine
      .query<string>('field_info')
      .then((i) =>
        Promise.all([
          i,
          engine.query<Float64Array>('field_x'),
          engine.query<Float64Array>('field_y'),
          engine.query<string>('equipotentials', Float64Array.from((JSON.parse(i) as FieldInfo).equipotential_levels)),
        ]),
      )
      .then(([i, x, y, eq]) => {
        if (stale) return;
        info = JSON.parse(i) as FieldInfo;
        gx = x;
        gy = y;
        const levels = JSON.parse(eq) as [number, [[number, number], [number, number]][]][];
        eqPath = levels
          .flatMap(([, segs]) => segs)
          .map(([a, b]) => `M${a[0]} ${a[1]}L${b[0]} ${b[1]}`)
          .join('');
        rendered = undefined;
        raster = undefined;
        error = '';
        showRegion(info.view_zoom);
      })
      .catch((e: unknown) => {
        if (!stale) error = e instanceof Error ? e.message : String(e);
      });
    return () => {
      stale = true;
    };
  });

  // ---- image, field lines: rendered for the visible region plus a margin, again when the view
  // leaves it or the zoom changes a lot
  interface Raster {
    canvas: HTMLCanvasElement;
    /** region the image covers (the requested one clipped to the grid) */
    region: Bounds;
    legend: Legend | null;
    lines: [number, number][][];
  }
  let raster = $state<Raster | undefined>();
  let rendered = $state<Rendered | undefined>();
  let rendering = false;

  $effect(() => {
    const i = info;
    if (!i || !canSample || gx.length < 2) return;
    const key = `${generation}|${mode}|${isDb ? dbRange : 0}|${units}`;
    const visible = bounds;
    if (rendering || !needsRender(rendered, key, visible)) return;
    rendering = true;
    const want = expand(visible, MARGIN);
    const width = Math.min(Math.max(Math.round(W * (1 + 2 * MARGIN)), 400), 2400);
    const wantArr = Float64Array.from(want);
    const [mode_, db, imperial, lines] = [mode, dbRange, units === 'imperial', hasLines];
    Promise.all([
      engine.query<RasterResult>('field_raster', mode_, wantArr, Float64Array.from(i.view_zoom), imperial, db, width),
      lines ? engine.query<string>('field_lines', wantArr, 2.2, 1.4) : Promise.resolve('[]'),
    ])
      .then(([r, ln]) => {
        const canvas = document.createElement('canvas');
        canvas.width = r.width;
        canvas.height = r.height;
        canvas.getContext('2d')?.putImageData(new ImageData(r.rgba as Uint8ClampedArray<ArrayBuffer>, r.width, r.height), 0, 0);
        // the image covers the wanted region clipped to the grid
        const region: Bounds = [
          Math.max(want[0], gx[0]!),
          Math.min(want[1], gx[gx.length - 1]!),
          Math.max(want[2], gy[0]!),
          Math.min(want[3], gy[gy.length - 1]!),
        ];
        raster = { canvas, region, legend: JSON.parse(r.legend) as Legend | null, lines: JSON.parse(ln) as [number, number][][] };
        rendered = { key, visible, covered: want };
        error = '';
      })
      .catch((e: unknown) => {
        error = e instanceof Error ? e.message : String(e);
        // do not retry in a loop: the next change of view or option tries again
        rendered = { key, visible, covered: want };
      })
      .finally(() => {
        rendering = false;
      });
  });

  // ---- triangle mesh of a finite-element solution: the edges around the visible region, asked for again when the
  // view leaves the region they cover
  const MESH_EDGE_LIMIT = 300000;
  let meshEdges = $state<{ covered: Bounds; segs: Float64Array; generation: number } | undefined>();
  let meshBusy = false;
  $effect(() => {
    const i = info;
    const visible = bounds;
    const g = generation;
    if (!i?.mesh || !meshOn || !canSample) return;
    const have = meshEdges;
    if (meshBusy) return;
    if (have && have.generation === g && contains(have.covered, visible)) {
      // enough, unless there were too many edges to draw and the view has since become much smaller
      const tooMany = have.segs.length / 4 > MESH_EDGE_LIMIT;
      const zoomedIn = (visible[1] - visible[0]) * (1 + 2 * MARGIN) < 0.8 * (have.covered[1] - have.covered[0]);
      if (!(tooMany && zoomedIn)) return;
    }
    meshBusy = true;
    const covered = expand(visible, MARGIN);
    engine
      .query<Float64Array>('field_mesh_edges', Float64Array.from(covered))
      .then((segs) => {
        meshEdges = { covered, segs, generation: g };
      })
      .catch((e: unknown) => {
        error = e instanceof Error ? e.message : String(e);
        meshEdges = { covered, segs: new Float64Array(), generation: g };
      })
      .finally(() => {
        meshBusy = false;
      });
  });
  const meshTooDense = $derived(!!meshEdges && meshEdges.segs.length / 4 > MESH_EDGE_LIMIT);
  const meshPath = $derived.by(() => {
    const m = meshEdges;
    if (!info?.mesh || !meshOn || !m || m.generation !== generation || meshTooDense) return '';
    const parts: string[] = [];
    const a = m.segs;
    for (let k = 0; k < a.length; k += 4) {
      parts.push(`M${px(a[k]!).toFixed(1)} ${py(a[k + 1]!).toFixed(1)}L${px(a[k + 2]!).toFixed(1)} ${py(a[k + 3]!).toFixed(1)}`);
    }
    return parts.join('');
  });

  // ---- paint the image
  let canvasEl = $state<HTMLCanvasElement | undefined>();
  $effect(() => {
    const el = canvasEl;
    const r = raster;
    const [w, h, , , v] = [PW, PH, W, H, view];
    if (!el) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = Math.round(w * dpr);
    el.height = Math.round(h * dpr);
    const ctx = el.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, el.width, el.height);
    if (!r) return;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';
    const [x0, x1, y0, y1] = r.region;
    const sx = (x: number) => (w / 2 + (x - v.cx) * v.ppm) * dpr;
    const sy = (y: number) => (h / 2 - (y - v.cy) * v.ppm) * dpr;
    ctx.drawImage(r.canvas, sx(x0), sy(y1), (x1 - x0) * v.ppm * dpr, (y1 - y0) * v.ppm * dpr);
  });

  // ---- vector overlay
  const xTicks = $derived(ticks(bounds[0] / uf, bounds[1] / uf, Math.max(Math.round(PW / 90), 3)));
  const yTicks = $derived(ticks(bounds[2] / uf, bounds[3] / uf, Math.max(Math.round(PH / 70), 3)));

  const linePath = (pts: [number, number][]) => pts.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${px(x).toFixed(1)} ${py(y).toFixed(1)}`).join('');
  /** small arrowhead in the middle of a field line */
  function arrow(pts: [number, number][]): string {
    if (pts.length < 3) return '';
    const m = pts.length >> 1;
    const [a, b, c] = [pts[m - 1]!, pts[m]!, pts[m + 1]!];
    const [dx, dy] = [px(c[0]) - px(a[0]), py(c[1]) - py(a[1])];
    const len = Math.hypot(dx, dy);
    if (!(len > 0)) return '';
    const [ux, uy, nx, ny] = [dx / len, dy / len, -dy / len, dx / len];
    const [tx, ty] = [px(b[0]) + ux * 3, py(b[1]) + uy * 3];
    return `M${tx - ux * 6 + nx * 2.5} ${ty - uy * 6 + ny * 2.5}L${tx} ${ty}L${tx - ux * 6 - nx * 2.5} ${ty - uy * 6 - ny * 2.5}`;
  }

  const gridBox = $derived<Bounds>(gx.length > 1 && gy.length > 1
    ? [Math.max(bounds[0], gx[0]!), Math.min(bounds[1], gx[gx.length - 1]!), Math.max(bounds[2], gy[0]!), Math.min(bounds[3], gy[gy.length - 1]!)]
    : bounds);

  // ---- interaction: wheel zooms around the cursor, drag pans, double click shows the signal region
  function wheel(node: SVGSVGElement) {
    const on = (e: WheelEvent) => {
      e.preventDefault();
      const r = node.getBoundingClientRect();
      const [x, y] = [((e.clientX - r.left) / r.width) * W - M.l, e.clientY - r.top - M.t];
      view = zoomAt(view, x, y, PW, PH, Math.exp(e.deltaY * 0.0015));
    };
    node.addEventListener('wheel', on, { passive: false });
    return { destroy: () => node.removeEventListener('wheel', on) };
  }

  let drag: { x: number; y: number } | undefined;
  let hover = $state<[number, number] | undefined>();

  function local(e: PointerEvent): [number, number] {
    const r = (e.currentTarget as Element).getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * W, e.clientY - r.top];
  }

  function down(e: PointerEvent) {
    const [x, y] = local(e);
    drag = { x, y };
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    const [x, y] = local(e);
    if (drag) {
      view = panBy(view, x - drag.x, y - drag.y);
      drag = { x, y };
    }
    hover = x >= M.l && x <= M.l + PW && y >= M.t && y <= M.t + PH ? [toX(x), toY(y)] : undefined;
  }

  function up(e: PointerEvent) {
    drag = undefined;
    (e.currentTarget as Element).releasePointerCapture(e.pointerId);
  }

  // ---- readout: values under the cursor, computed by the module
  let probe = $state<Probe | null>(null);
  let probeSeq = 0;
  $effect(() => {
    const h = hover;
    if (!h || !canSample || !info) {
      probe = null;
      return;
    }
    const mine = ++probeSeq;
    engine
      .query<string>('field_probe', h[0], h[1])
      .then((s) => {
        if (mine === probeSeq) probe = JSON.parse(s) as Probe | null;
      })
      .catch(() => {});
  });

  const readout = $derived.by(() => {
    if (!hover) return 'Move the cursor over the plot for the values there. Wheel zooms, drag pans, double click shows the signal region.';
    let s = `x = ${formatSig(hover[0] / uf, 5)} ${lengthUnit[0]}   y = ${formatSig(hover[1] / uf, 5)} ${lengthUnit[0]}`;
    if (probe?.in_conductor) s += '   (conductor)';
    else if (probe) {
      s += `   V = ${formatSig(probe.v, 4)} V`;
      if (probe.e !== null) {
        const db = 20 * Math.log10(Math.max(probe.e * fieldPerVPerM(units), 1e-30));
        s += `   |E| = ${formatField(units, probe.e)} (${db.toFixed(1)} ${fieldDbUnit(units)})   er = ${probe.er}`;
      } else s += `   er = ${probe.er}`;
    }
    return s;
  });

  const title = $derived(`${LABELS[mode]}  -  ${info ? info.drive.replace('Ohm', 'Ω') : '…'}`);
  /** the shortest edge of a triangle mesh is far below the size of the view: its own unit */
  const minUnit = $derived(autoLengthUnit(units, accuracy?.dmin_actual ?? 1));
  const meshInfo = $derived(
    meshOn && accuracy
      ? `mesh ${accuracy.nodes} nodes (finite elements), min. edge ${formatSig(accuracy.dmin_actual / minUnit[1], 3)} ${minUnit[0]}${meshTooDense ? ' - zoom in to see the triangles' : ''}`
      : ' ',
  );
</script>

<div class="controls">
  <select bind:value={mode} aria-label="Field view">
    {#each FIELD_MODES as m}
      <option value={m}>{LABELS[m]}</option>
    {/each}
  </select>
  <label class:off={!allowsEq}><input type="checkbox" bind:checked={showEq} disabled={!allowsEq} /> Equipotentials</label>
  <label class:off={mode === 'mesh'}><input type="checkbox" bind:checked={showMesh} disabled={mode === 'mesh'} /> Mesh overlay</label>
  {#if isDb}
    <label>
      Range
      <input type="number" min="5" max="200" step="1" bind:value={dbRange} class="range" /> dB
    </label>
  {/if}
  <button disabled={!info} onclick={() => info && showRegion(info.view_zoom)}>Signal region</button>
  <button disabled={!info} onclick={() => info && showRegion(info.view_full)}>Full view</button>
</div>

<p class="title">{title}<br />{meshInfo}</p>

{#if !canSample}
  <p class="hint">The last calculation was cancelled, so there is no field to show. Press Analyze again.</p>
{:else if !info && !error}
  <p class="hint">Loading the field…</p>
{:else}
  <div class="row">
    <div class="box" bind:clientWidth={boxW} bind:clientHeight={boxH}>
      <canvas bind:this={canvasEl} style:left="{M.l}px" style:top="{M.t}px" style:width="{PW}px" style:height="{PH}px"></canvas>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <svg
        width={W}
        height={H}
        role="img"
        aria-label="Field in the cross-section"
        use:wheel
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointerleave={() => (hover = undefined)}
        ondblclick={() => info && showRegion(info.view_zoom)}
      >
        <defs><clipPath id="field-clip"><rect x="0" y="0" width={PW} height={PH} /></clipPath></defs>
        <g transform="translate({M.l} {M.t})">
          <rect x="0" y="0" width={PW} height={PH} class="frame" />
          {#each xTicks as t}
            <text x={px(t * uf)} y={PH + 16} text-anchor="middle" class="tick">{formatG(t)}</text>
            <line x1={px(t * uf)} x2={px(t * uf)} y1={PH} y2={PH + 4} class="tickmark" />
          {/each}
          {#each yTicks as t}
            <text x="-6" y={py(t * uf)} text-anchor="end" dominant-baseline="central" class="tick">{formatG(t)}</text>
            <line x1="-4" x2="0" y1={py(t * uf)} y2={py(t * uf)} class="tickmark" />
          {/each}
          <text x={PW / 2} y={PH + 36} text-anchor="middle" class="axis">x [{lengthUnit[0]}]</text>
          <text transform="translate(-54 {PH / 2}) rotate(-90)" text-anchor="middle" class="axis">y [{lengthUnit[0]}]  (0 = signal layer)</text>

          <g clip-path="url(#field-clip)">
            {#if hasLines && raster}
              {#each raster.lines as pts}
                <path d={linePath(pts)} class="fline" />
                <path d={arrow(pts)} class="fline" />
              {/each}
            {/if}
            {#if showEq && allowsEq && eqPath}
              <g transform="translate({PW / 2 - view.cx * view.ppm} {PH / 2 + view.cy * view.ppm}) scale({view.ppm} {-view.ppm})">
                <path d={eqPath} class="eq" vector-effect="non-scaling-stroke" />
              </g>
            {/if}
            {#if info}
              {#each info.conductors as c}
                {@const top = c.top ?? [c.x0, c.x1]}
                <polygon points="{px(c.x0)},{py(c.y0)} {px(c.x1)},{py(c.y0)} {px(top[1])},{py(c.y1)} {px(top[0])},{py(c.y1)}" fill={COPPER} class="cond" />
              {/each}
              {#each info.dashed_y as y}
                <line x1={px(gridBox[0])} x2={px(gridBox[1])} y1={py(y)} y2={py(y)} class="guide dashed" />
              {/each}
              {#each info.dotted_y as y}
                <line x1={px(gridBox[0])} x2={px(gridBox[1])} y1={py(y)} y2={py(y)} class="guide dotted" />
              {/each}
            {/if}
            {#if meshOn}
              <g class:meshdark={mode === 'mesh'} class="mesh">
                {#if meshPath}
                  <path d={meshPath} class="tri" />
                {/if}
              </g>
            {/if}
            {#if hover}
              <circle cx={px(hover[0])} cy={py(hover[1])} r="3" class="cursor" />
            {/if}
          </g>
        </g>
      </svg>
    </div>
    {#if raster?.legend}
      <ColorBar legend={raster.legend} top={M.t} height={PH} />
    {:else}
      <div class="nobar"></div>
    {/if}
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <p class="readout">{readout}</p>
{/if}

<style>
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    margin-bottom: 8px;
  }
  .controls label {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .off {
    opacity: 0.5;
  }
  .range {
    width: 4.5rem;
    background: var(--field);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 3px 5px;
  }
  .title {
    margin: 0 0 6px;
    min-height: 2.8em;
  }
  .hint {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
  .row {
    flex: 1 1 380px;
    min-height: 380px;
    display: flex;
    align-items: stretch;
  }
  .nobar {
    flex: none;
    width: 120px;
  }
  .box {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  canvas {
    position: absolute;
    background: var(--panel);
  }
  svg {
    position: absolute;
    inset: 0;
    touch-action: none;
    cursor: crosshair;
    user-select: none;
  }
  .frame {
    fill: none;
    stroke: var(--border);
  }
  .tick {
    font-size: 11px;
    fill: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .tickmark {
    stroke: var(--muted);
  }
  .axis {
    font-size: 12px;
    fill: var(--text);
  }
  .fline {
    fill: none;
    stroke: #9fd3ff;
    stroke-width: 0.8;
  }
  .eq {
    fill: none;
    stroke: rgb(255 255 255 / 0.78);
    stroke-width: 0.8;
  }
  .cond {
    stroke: #000;
    stroke-width: 0.5;
  }
  .guide {
    stroke: #fff;
  }
  .guide.dashed {
    stroke-opacity: 0.78;
    stroke-width: 0.8;
    stroke-dasharray: 6 6;
  }
  .guide.dotted {
    stroke-opacity: 0.35;
    stroke-width: 0.5;
    stroke-dasharray: 2 2;
  }
  .mesh path.tri {
    fill: none;
    stroke: rgb(255 255 255 / 0.5);
    stroke-width: 0.4;
  }
  .meshdark path.tri {
    stroke: rgb(0 0 0 / 0.6);
  }
  .cursor {
    fill: none;
    stroke: #fff;
    stroke-width: 1;
  }
  .readout {
    margin: 8px 0 0;
    font-family: var(--mono);
    font-size: 12.5px;
    min-height: 2.8em;
    white-space: pre-wrap;
  }
</style>
