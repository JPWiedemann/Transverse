import { describe, expect, it } from 'vitest';
import { allBounds, fitBounds, profileValue, readoutText, segments, yBottom, yFit } from './returncurrent';
import type { CurrentProfile, SplitSummary } from './types';

const profile = (broken: boolean): CurrentProfile => ({
  key: 'k',
  label: 'l',
  share_pct: 10,
  x: [-2, -1, 1, 2],
  j: [1, 2, 4, 1],
  color: 0,
  dashed: false,
  break_at_zero: broken,
});
const split = (...profiles: CurrentProfile[]): SplitSummary => ({
  rows: [],
  x90: [],
  profiles,
  plot: { title: '', half_width: 1.5, signals: [], markers: [], labels: [] },
  info: '',
});

describe('return current helpers', () => {
  it('breaks profiles across the signal only when asked', () => {
    expect(segments(profile(false)).map((s) => s.length)).toEqual([4]);
    expect(segments(profile(true)).map((s) => s.length)).toEqual([2, 2]);
  });

  it('interpolates and respects gaps', () => {
    const [cont, broken] = [profile(false), profile(true)];
    expect(profileValue(cont, -1.5)).toBe(1.5);
    expect(profileValue(broken, 1.5)).toBe(2.5);
    expect(profileValue(broken, -1)).toBe(2);
    expect(profileValue(broken, 2)).toBe(1);
    expect(profileValue(broken, -2)).toBe(1);
    expect(profileValue(cont, 0)).toBe(3);
    expect(profileValue(broken, 0)).toBeUndefined();
    expect(profileValue(cont, -2.5)).toBeUndefined();
    expect(profileValue(cont, 2.5)).toBeUndefined();
    expect(profileValue({ ...profile(false), x: [], j: [] }, 0)).toBeUndefined();
    const single = { ...profile(false), x: [1], j: [5] };
    expect(profileValue(single, 1)).toBe(5);
    expect(profileValue(single, 1.1)).toBeUndefined();
  });

  it('reads out in the chosen length unit', () => {
    const s = split(profile(false));
    expect(readoutText(s, -1.5, 'mm', 1)).toBe('x = -1.5000 mm    J_s/I [1/mm]    l: 1.500');
    const i = readoutText(s, -1.5, 'mil', 0.0254);
    expect(i.startsWith('x = -59.055 mil    J_s/I [1/mil]')).toBe(true);
    expect(i.endsWith('l: 0.03810')).toBe(true);
    expect(readoutText(s, 5, 'mm', 1).endsWith('l: -')).toBe(true);
    expect(readoutText(split(profile(true)), 0, 'mm', 1).endsWith('l: -')).toBe(true);
  });

  it('fits y to the curves inside the visible x range', () => {
    const s = split(profile(false));
    expect(yFit(s, -2, 2)).toBeCloseTo(1.15 * 4, 12);
    expect(yFit(s, -2, -0.5)).toBeCloseTo(1.15 * 2.5, 12);
    expect(yFit(s, -2, -1)).toBeCloseTo(1.15 * 2, 12);
    expect(yFit(s, -0.5, 0.5)).toBeCloseTo(1.15 * 3.5, 12);
    expect(yFit(split(), 0, 1)).toBe(1);
  });

  it('has fit and show-all bounds', () => {
    const s = split(profile(false));
    // the peak 4 is at |x| = 1 (inside), the end value 1 at |x| = 2 (outside the fit range)
    expect(fitBounds(s)).toEqual([-1.5, 1.5, 0, 1.15 * 4]);
    expect(allBounds(s)).toEqual([-2, 2, 0, 1.15 * 4]);
    const p = profile(false);
    p.j = [1, 1, 1, 3];
    expect(fitBounds(split(p))[3]).toBeCloseTo(1.15, 12);
    expect(allBounds(split(p))[3]).toBeCloseTo(1.15 * 3, 12);
    expect(fitBounds(split())[3]).toBe(1);
  });
});

describe('currents of both signs (odd mode of a pair)', () => {
  const odd: CurrentProfile = { ...profile(false), x: [-1, -0.5, 0.5, 1], j: [1, 3, -3, -1] };
  const s: SplitSummary = { ...split(odd), plot: { ...split(odd).plot, half_width: 1.5 } };

  it('are inside the plot', () => {
    expect(fitBounds(s)).toEqual([-1.5, 1.5, -1.15 * 3, 1.15 * 3]);
    expect(allBounds(s)[2]).toBe(-1.15 * 3);
    expect(yBottom(s, -1, 0)).toBe(0);
  });

  it('are not assumed for a trace of negative noise', () => {
    expect(fitBounds(split({ ...profile(false), j: [1, 3, 2, -0.01] }))[2]).toBe(0);
  });
});
