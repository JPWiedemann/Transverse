import { describe, expect, it } from 'vitest';
import { FormError, changeUnit, controlSet, applyPreset, isVisible, loadSaved, newForm, toParams, toSaved } from './form';
import type { ParamSet, ParamSpec } from './types';

const specs: ParamSpec[] = [
  { key: 'w', label: 'Trace width w', group: 'G', kind: { Length: { default_mm: 0.3605, unit: 'mm', lambda: false } }, visible_when: [] },
  { key: 'dmin', label: 'Min. cell', group: 'G', kind: { Length: { default_mm: 0.002, unit: 'µm', lambda: true } }, visible_when: [[{ key: 'mode', value: 1 }]] },
  { key: 'f', label: 'Frequency', group: 'G', kind: { Frequency: { default_hz: 1e9, unit: 'GHz' } }, visible_when: [] },
  { key: 'sigma', label: 'Conductivity', group: 'G', kind: { Number: { default: 5.8e7, unit: 'MS/m', scale: 1e6 } }, visible_when: [] },
  { key: 'plane', label: 'Plane', group: 'G', kind: { Bool: true }, visible_when: [] },
  { key: 'mode', label: 'Mode', group: 'G', kind: { Choice: [['auto', 'manual'], 0] }, visible_when: [] },
];
const defaults: ParamSet = { w: 0.3605, dmin: 0.002, f: 1e9, sigma: 5.8e7, plane: true, mode: 0 };
// a made-up wavelength that depends on f, like the real one
const wl = (s: ParamSet) => (typeof s.f === 'number' ? 3e11 / s.f : undefined);

describe('form', () => {
  it('shows defaults in their display units and converts back', () => {
    const f = newForm(specs, defaults, wl);
    expect(f.w).toEqual({ kind: 'quantity', text: '0.3605', unit: 'mm' });
    expect(f.dmin).toEqual({ kind: 'quantity', text: '2', unit: 'µm' });
    expect(f.sigma).toEqual({ kind: 'quantity', text: '58', unit: 'MS/m' });
    const p = toParams(specs, defaults, f, wl);
    expect(p.w).toBeCloseTo(0.3605, 12);
    expect(p.dmin).toBeCloseTo(0.002, 12);
    expect(p.sigma).toBeCloseTo(5.8e7, 3);
    expect(p.plane).toBe(true);
  });

  it('reports the offending field', () => {
    const f = newForm(specs, defaults, wl);
    f.w = { kind: 'quantity', text: 'abc', unit: 'mm' };
    expect(() => toParams(specs, defaults, f, wl)).toThrowError(FormError);
    expect(() => toParams(specs, defaults, f, wl)).toThrowError("Trace width w: 'abc' is not a number");
  });

  it('converts a changed unit and keeps unparsable text', () => {
    let f = newForm(specs, defaults, wl);
    f = changeUnit(specs, defaults, f, 'w', 'mil', wl);
    expect(f.w).toEqual({ kind: 'quantity', text: '14.1929', unit: 'mil' });
    f.w = { kind: 'quantity', text: 'x', unit: 'mil' };
    f = changeUnit(specs, defaults, f, 'w', 'in', wl);
    expect(f.w).toEqual({ kind: 'quantity', text: 'x', unit: 'in' });
  });

  it('sizes in λ follow the frequency', () => {
    let f = newForm(specs, defaults, wl);
    f = changeUnit(specs, defaults, f, 'dmin', 'λ', wl);
    const p = toParams(specs, defaults, f, wl);
    // the text keeps six significant digits
    expect(Math.abs((p.dmin as number) / 0.002 - 1)).toBeLessThan(1e-5);
    f.f = { kind: 'quantity', text: '2', unit: 'GHz' };
    // the same text in λ is now half as long
    expect((toParams(specs, defaults, f, wl).dmin as number) / (p.dmin as number)).toBeCloseTo(0.5, 12);
  });

  it('shows fields by flags and choices, in alternatives', () => {
    const f = newForm(specs, defaults, wl);
    const dmin = specs[1]!.visible_when;
    expect(isVisible(dmin, controlSet(defaults, f))).toBe(false);
    f.mode = { kind: 'choice', index: 1 };
    expect(isVisible(dmin, controlSet(defaults, f))).toBe(true);
    expect(isVisible([], controlSet(defaults, f))).toBe(true);
    const either = [[{ key: 'mode', value: 0 }], [{ key: 'plane', value: 1 }, { key: 'mode', value: 1 }]];
    expect(isVisible(either, { mode: 1, plane: true })).toBe(true);
    expect(isVisible(either, { mode: 1, plane: false })).toBe(false);
  });

  it('does not let a hidden field block, and presets keep the other fields', () => {
    const f = newForm(specs, defaults, wl);
    f.dmin = { kind: 'quantity', text: 'abc', unit: 'µm' };
    // hidden in the automatic mode: the default is used, the text stays
    expect(toParams(specs, defaults, f, wl).dmin).toBe(defaults.dmin);
    const manual = applyPreset(f, { id: 'm', label: 'Manual', values: [['mode', 1], ['plane', false], ['nope', true]] });
    expect(manual.mode).toEqual({ kind: 'choice', index: 1 });
    expect(manual.plane).toEqual({ kind: 'flag', value: false });
    expect(manual.dmin).toEqual(f.dmin);
    expect(manual.nope).toBeUndefined();
    expect(() => toParams(specs, defaults, manual, wl)).toThrow('not a number');
  });

  it('saves and restores, ignoring invalid entries', () => {
    const f = newForm(specs, defaults, wl);
    f.w = { kind: 'quantity', text: '0.5', unit: 'mil' };
    f.mode = { kind: 'choice', index: 1 };
    const g = loadSaved(specs, newForm(specs, defaults, wl), JSON.parse(JSON.stringify(toSaved(f))));
    expect(g).toEqual(f);
    const bad = loadSaved(specs, newForm(specs, defaults, wl), { w: { unit: 'parsec' }, mode: { choice: 7 }, sigma: { unit: 'S/m' } });
    expect(bad).toEqual(newForm(specs, defaults, wl));
    expect(loadSaved(specs, f, null)).toEqual(f);
  });
});
