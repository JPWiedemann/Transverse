import { describe, expect, it } from 'vitest';
import { boundsOf, colourbarScale, contains, expand, fitView, needsRender, panBy, zoomAt } from './fieldview';

describe('field view', () => {
  it('fits a region and gives it back', () => {
    const v = fitView([0, 2, 0, 1], 400, 400);
    expect(v.ppm).toBe(200); // the width is the limit
    const b = boundsOf(v, 400, 400);
    expect(b[0]).toBeCloseTo(0, 12);
    expect(b[1]).toBeCloseTo(2, 12);
    expect(b[3] - b[2]).toBeCloseTo(2, 12); // the extra height is shown too
    expect((b[2] + b[3]) / 2).toBeCloseTo(0.5, 12);
  });

  it('zooms around the cursor, which keeps its place', () => {
    const v = fitView([0, 2, 0, 1], 400, 200);
    const at = (view: typeof v, px: number, py: number): [number, number] => [
      view.cx + (px - 200) / view.ppm,
      view.cy - (py - 100) / view.ppm,
    ];
    const z = zoomAt(v, 300, 40, 400, 200, 0.5);
    expect(z.ppm).toBeCloseTo(v.ppm * 2, 12);
    const [a, b] = [at(v, 300, 40), at(z, 300, 40)];
    expect(b[0]).toBeCloseTo(a[0], 12);
    expect(b[1]).toBeCloseTo(a[1], 12);
  });

  it('pans with the image following the pointer', () => {
    const v = { cx: 0, cy: 0, ppm: 100 };
    // dragging right by 100 px moves the content right: the centre moves left by 1 mm
    expect(panBy(v, 100, 0)).toEqual({ cx: -1, cy: 0, ppm: 100 });
    // dragging down moves the content down: the centre moves up
    expect(panBy(v, 0, 100)).toEqual({ cx: 0, cy: 1, ppm: 100 });
  });

  it('knows when the image has to be rendered again', () => {
    const visible: [number, number, number, number] = [0, 1, 0, 1];
    const r = { key: 'a', visible, covered: expand(visible, 0.25) };
    expect(contains(r.covered, visible)).toBe(true);
    expect(needsRender(undefined, 'a', visible)).toBe(true);
    expect(needsRender(r, 'b', visible)).toBe(true); // other mode or result
    expect(needsRender(r, 'a', visible)).toBe(false);
    expect(needsRender(r, 'a', [0.1, 1.1, 0.1, 1.1])).toBe(false); // small pan stays inside
    expect(needsRender(r, 'a', [0.5, 1.5, 0, 1])).toBe(true); // leaves the image
    expect(needsRender(r, 'a', [0.2, 0.8, 0.2, 0.8])).toBe(false); // ratio 0.6 is still fine
    expect(needsRender(r, 'a', [0.3, 0.7, 0.3, 0.7])).toBe(true); // zoomed in too far
    expect(needsRender(r, 'a', [-0.1, 1.1, -0.1, 1.1])).toBe(false); // ratio 1.2 is fine
    expect(needsRender(r, 'a', [-0.25, 1.25, -0.25, 1.25])).toBe(true); // ratio 1.5: zoomed out too far
  });

  it('scales the colour bar', () => {
    const db = { label: '|E| [dBV/m]', vmin: 0, vmax: 100, cmap: 'Inferno', field_base: null } as const;
    expect(colourbarScale(db)).toEqual({ title: '|E| [dBV/m]', mult: 1 });
    const lin = { label: 'x', vmin: 0, vmax: 15340, cmap: 'Inferno', field_base: 'V/m' } as const;
    expect(colourbarScale(lin)).toEqual({ title: '|E| [kV/m]', mult: 1e-3 });
  });
});
