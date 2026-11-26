import { describe, expect, it } from 'vitest';
import { niceStep, ticks, zoomRange } from './plot';

describe('plot helpers', () => {
  it('picks 1, 2, 5 steps', () => {
    expect(niceStep(0.3)).toBe(0.5);
    expect(niceStep(1)).toBe(1);
    expect(niceStep(1.1)).toBe(2);
    expect(niceStep(7)).toBe(10);
    expect(niceStep(0.0031)).toBeCloseTo(0.005, 12);
  });

  it('makes round ticks inside the range', () => {
    expect(ticks(0, 20, 5)).toEqual([0, 5, 10, 15, 20]);
    const t = ticks(0.0012, 0.0087, 6);
    expect(t[0]).toBeGreaterThanOrEqual(0.0012);
    expect(t.at(-1)).toBeLessThanOrEqual(0.0087);
    expect(t.length).toBeGreaterThan(2);
    expect(ticks(1, 1)).toEqual([]);
    expect(ticks(0, Number.NaN)).toEqual([]);
  });

  it('zooms around a point, which stays where it is', () => {
    const [lo, hi] = zoomRange(0, 10, 2, 0.5);
    expect([lo, hi]).toEqual([1, 6]);
    expect(zoomRange(0, 10, 0, 2)).toEqual([0, 20]);
  });
});
