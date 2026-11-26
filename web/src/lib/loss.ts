// Loss-versus-frequency data: sweep, curves, plot range and readout. Port of the helpers of
// transverse-gui/views/loss.rs; the numbers come from the wasm module (`sample`).

import { SAMPLE_COLUMNS } from './engine';
import { FREQ_UNITS, factor, formatG, parseNumber } from './units';

export const POINTS = 400;
export const DEFAULT_FMAX_HZ = 20e9;
/** Extra room to the right when the set frequency is at or above f max. */
export const RANGE_MARGIN = 1.02;
/** Np/m -> dB/m is 20 / ln(10) (same as transverse_core::NP2DB). */
export const NP2DB = 20 / Math.LN10;
export const MM_PER_INCH = 25.4;

export type UnitSystem = 'metric' | 'imperial';

/** `n` frequencies from `fmax / n` to `fmax`. */
export function sweep(fmaxHz: number, n: number): number[] {
  return Array.from({ length: n }, (_, k) => (fmaxHz * (k + 1)) / n);
}

/** The "f max" entry [Hz]; an invalid entry falls back to 20 GHz. */
export function fmaxHz(text: string, unit: string): number {
  const v = parseNumber(text);
  const f = factor(FREQ_UNITS, unit, undefined);
  return v !== undefined && f !== undefined && v * f > 0 ? v * f : DEFAULT_FMAX_HZ;
}

/** Upper end of the plot: the entry, unless the set frequency is at or above it. Then the plot
 * extends 2 % past that frequency, so its marker is never exactly on the edge. */
export function effectiveFmaxHz(entryHz: number, floorHz: number): number {
  return Number.isFinite(floorHz) && floorHz > 0 && floorHz >= entryHz ? floorHz * RANGE_MARGIN : entryHz;
}

export interface Curves {
  /** GHz */
  f: number[];
  /** dB/mm */
  conductor: number[];
  dielectric: number[];
  total: number[];
}

/** Rows of `SAMPLE_COLUMNS` per frequency -> curves in dB/mm. */
export function curvesFrom(freqsHz: number[], data: Float64Array): Curves {
  const n = SAMPLE_COLUMNS.length;
  const ic = SAMPLE_COLUMNS.indexOf('alpha_c');
  const id = SAMPLE_COLUMNS.indexOf('alpha_d');
  const toDbMm = NP2DB / 1000;
  const out: Curves = { f: [], conductor: [], dielectric: [], total: [] };
  freqsHz.forEach((f, k) => {
    const c = (data[k * n + ic] ?? 0) * toDbMm;
    const d = (data[k * n + id] ?? 0) * toDbMm;
    out.f.push(f / 1e9);
    out.conductor.push(c);
    out.dielectric.push(d);
    out.total.push(c + d);
  });
  return out;
}

/** Top of the y range that fits `totals` (dB/mm): 5 % above the maximum. */
export function yTop(totals: number[]): number {
  const m = totals.reduce((a, b) => Math.max(a, b), 0);
  return m > 0 ? 1.05 * m : 1;
}

/** (primary unit, factor from dB/mm, secondary unit, factor from dB/mm) */
export function lossAxes(units: UnitSystem): { label: string; k: number; other: string; kOther: number } {
  return units === 'metric'
    ? { label: 'Loss [dB/mm]', k: 1, other: 'dB/in', kOther: MM_PER_INCH }
    : { label: 'Loss [dB/in]', k: MM_PER_INCH, other: 'dB/mm', kOther: 1 };
}

/** Cursor readout for one sample row at `fGhz`: the unit system's unit first. */
export function readoutText(row: ArrayLike<number>, fGhz: number, units: UnitSystem): string {
  const toDbMm = NP2DB / 1000;
  const ac = (row[SAMPLE_COLUMNS.indexOf('alpha_c')] ?? 0) * toDbMm;
  const ad = (row[SAMPLE_COLUMNS.indexOf('alpha_d')] ?? 0) * toDbMm;
  const { k, kOther, other } = lossAxes(units);
  const u1 = units === 'metric' ? 'dB/mm' : 'dB/in';
  const v = (name: string, dbMm: number) =>
    `${name}: ${(dbMm * k).toFixed(5)} ${u1} (${(dbMm * kOther).toFixed(4)} ${other})`;
  return `f = ${formatG(fGhz)} GHz    ${v('Conductor', ac)}    ${v('Dielectric', ad)}    ${v('Total', ac + ad)}`;
}
