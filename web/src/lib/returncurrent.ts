// Return-current tab: plot ranges, profile values and readout. Port of the helpers of
// transverse-gui/views/return_current.rs.

import type { CurrentProfile, SplitSummary } from './types';
import { formatSig } from './units';

/** [x0, x1, y0, y1] in mm and 1/mm */
export type Bounds = [number, number, number, number];

/** Upper limit of the plot: 15 % above the largest value with |x| <= `limit`. */
function yTop(split: SplitSummary, limit: number): number {
  let m = 0;
  for (const p of split.profiles) {
    p.x.forEach((x, i) => {
      const j = p.j[i] ?? Number.NaN;
      if (Math.abs(x) <= limit && Number.isFinite(j)) m = Math.max(m, j);
    });
  }
  return m > 0 ? 1.15 * m : 1;
}

/** Lower limit of the plot for `x0..x1`: 0, or 15 % below the smallest value there if the current has both signs
 * (the odd mode of a pair). Values that are negative only by a trace do not count. */
export function yBottom(split: SplitSummary, x0: number, x1: number): number {
  let low = 0;
  let high = 0;
  for (const p of split.profiles) {
    p.x.forEach((x, i) => {
      const j = p.j[i] ?? Number.NaN;
      if (x >= x0 && x <= x1 && Number.isFinite(j)) [low, high] = [Math.min(low, j), Math.max(high, j)];
    });
  }
  return -low > 0.02 * high ? 1.15 * low : 0;
}

/** The view the plot starts with and "Signal region" returns to. */
export function fitBounds(split: SplitSummary): Bounds {
  const hw = split.plot.half_width;
  return [-hw, hw, yBottom(split, -hw, hw), yTop(split, hw)];
}

/** Everything the profiles contain. */
export function allBounds(split: SplitSummary): Bounds {
  let lo = Infinity;
  let hi = -Infinity;
  for (const p of split.profiles) for (const x of p.x) [lo, hi] = [Math.min(lo, x), Math.max(hi, x)];
  return lo < hi ? [lo, hi, yBottom(split, lo, hi), yTop(split, Infinity)] : fitBounds(split);
}

/** Value of a profile at `x` by linear interpolation; undefined outside its range or across the
 * signal (where a profile that is broken at zero is not defined). */
export function profileValue(p: CurrentProfile, x: number): number | undefined {
  const n = p.x.length;
  const first = p.x[0];
  const last = p.x[n - 1];
  if (first === undefined || last === undefined || x < first || x > last) return undefined;
  if (n < 2) return x === first ? p.j[0] : undefined;
  // first index with p.x[i] > x, clamped to 1..n-1
  let lo = 0;
  let hi = n;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((p.x[mid] as number) <= x) lo = mid + 1;
    else hi = mid;
  }
  const i = Math.min(Math.max(lo, 1), n - 1);
  const [x0, x1] = [p.x[i - 1] as number, p.x[i] as number];
  const [j0, j1] = [p.j[i - 1] as number, p.j[i] as number];
  if (x === x0) return j0; // exactly on a point, also next to a gap
  if (p.break_at_zero && x0 < 0 && x1 > 0) return undefined;
  const t = x1 > x0 ? (x - x0) / (x1 - x0) : 0;
  return j0 + t * (j1 - j0);
}

/** Top of the y range that fits the curves inside `x0..x1`: 15 % above their maximum there,
 * evaluated at the points and at both edges. */
export function yFit(split: SplitSummary, x0: number, x1: number): number {
  let m = 0;
  for (const p of split.profiles) {
    p.x.forEach((x, i) => {
      const j = p.j[i] ?? Number.NaN;
      if (x >= x0 && x <= x1 && Number.isFinite(j)) m = Math.max(m, j);
    });
    for (const x of [x0, x1]) {
      const v = profileValue(p, x);
      if (v !== undefined && Number.isFinite(v)) m = Math.max(m, v);
    }
  }
  return m > 0 ? 1.15 * m : 1;
}

/** The points of a profile as one or two lines (two when it is not defined across the signal). */
export function segments(p: CurrentProfile): [number, number][][] {
  const pts = (keep: (x: number) => boolean) =>
    p.x.flatMap((x, i): [number, number][] => (keep(x) ? [[x, p.j[i] ?? 0]] : []));
  return p.break_at_zero ? [pts((x) => x < 0), pts((x) => x > 0)] : [pts(() => true)];
}

/** Values of all curves at `xMm`, in `unit` (`uf` mm per unit). */
export function readoutText(split: SplitSummary, xMm: number, unit: string, uf: number): string {
  const vals = split.profiles.map((p) => {
    const v = profileValue(p, xMm);
    return `${p.label}: ${v === undefined ? '-' : formatSig(v * uf, 4)}`;
  });
  return `x = ${formatSig(xMm / uf, 5)} ${unit}    J_s/I [1/${unit}]    ${vals.join('    ')}`;
}
