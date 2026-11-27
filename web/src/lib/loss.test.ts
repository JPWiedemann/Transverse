import { describe, expect, it } from 'vitest';
import { NP2DB, curvesFrom, effectiveFmaxHz, fmaxHz, lossAxes, readoutText, sweep, yTop } from './loss';

describe('loss helpers', () => {
  it('sweeps from fmax/n to fmax', () => {
    const s = sweep(20e9, 400);
    expect(s).toHaveLength(400);
    expect(s[0]).toBeCloseTo(20e9 / 400, 3);
    expect(s.at(-1)).toBe(20e9);
  });

  it('parses the f max entry with its unit and falls back when invalid', () => {
    expect(fmaxHz('20', 'GHz')).toBe(20e9);
    expect(fmaxHz('500', 'MHz')).toBe(500e6);
    expect(fmaxHz('5,5', 'GHz')).toBe(5.5e9);
    for (const bad of ['', 'abc', '-3', '0']) expect(fmaxHz(bad, 'GHz')).toBe(20e9);
  });

  it('leaves room to the right of the set frequency', () => {
    expect(effectiveFmaxHz(5e9, 2e9)).toBe(5e9);
    expect(effectiveFmaxHz(5e9, 4.9e9)).toBe(5e9);
    expect(effectiveFmaxHz(5e9, 12e9)).toBeCloseTo(12.24e9, 0);
    expect(effectiveFmaxHz(5e9, 5e9)).toBeCloseTo(5.1e9, 0);
    for (const f of [1e9, 5e9, 5.01e9, 12e9, 1e12]) expect(effectiveFmaxHz(5e9, f)).toBeGreaterThanOrEqual(f);
    for (const bad of [Number.NaN, -1, 0]) expect(effectiveFmaxHz(5e9, bad)).toBe(5e9);
  });

  it('converts samples to dB/mm', () => {
    // one row: only alpha_c = 1 Np/m and alpha_d = 2 Np/m matter
    const row = new Float64Array(9);
    row[2] = 1;
    row[3] = 2;
    const c = curvesFrom([1e9], row);
    expect(c.f).toEqual([1]);
    expect(c.conductor[0]).toBeCloseTo(NP2DB / 1000, 15);
    expect(c.total[0]).toBeCloseTo((3 * NP2DB) / 1000, 15);
    expect(yTop(c.total)).toBeCloseTo((1.05 * 3 * NP2DB) / 1000, 15);
    expect(yTop([])).toBe(1);
  });

  it('leads the readout with the unit of the selected system', () => {
    const row = new Float64Array(9);
    row[2] = 100;
    row[3] = 50;
    const total = (150 * NP2DB) / 1000;
    expect(readoutText(row, 2, 'metric')).toContain(`Total: ${total.toFixed(5)} dB/mm (${(total * 25.4).toFixed(4)} dB/in)`);
    expect(readoutText(row, 2, 'imperial')).toContain(`Total: ${(total * 25.4).toFixed(5)} dB/in (${total.toFixed(4)} dB/mm)`);
    expect(readoutText(row, 2, 'metric').startsWith('f = 2 GHz')).toBe(true);
    expect(lossAxes('imperial').label).toBe('Loss [dB/in]');
  });
});
