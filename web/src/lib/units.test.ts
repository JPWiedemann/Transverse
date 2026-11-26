import { describe, expect, it } from 'vitest';
import { FREQ_UNITS, LEN_UNITS, factor, formatG, parseNumber } from './units';

describe('units', () => {
  it('finds factors and handles λ', () => {
    expect(factor(LEN_UNITS, 'mil', undefined)).toBe(0.0254);
    expect(factor(FREQ_UNITS, 'GHz', undefined)).toBe(1e9);
    expect(factor(LEN_UNITS, 'λ', undefined)).toBeUndefined();
    expect(factor(LEN_UNITS, 'λ', 50)).toBe(50);
    expect(factor(LEN_UNITS, 'parsec', undefined)).toBeUndefined();
  });

  it('parses decimal comma and rejects junk', () => {
    expect(parseNumber(' 0,3605 ')).toBe(0.3605);
    expect(parseNumber('1e-3')).toBe(0.001);
    expect(parseNumber('')).toBeUndefined();
    expect(parseNumber('abc')).toBeUndefined();
    expect(parseNumber('Infinity')).toBeUndefined();
  });

  it('formats like {:.6g}', () => {
    expect(formatG(0.3605)).toBe('0.3605');
    expect(formatG(100)).toBe('100');
    expect(formatG(1 / 3)).toBe('0.333333');
    expect(formatG(5.8e7)).toBe('5.8e7');
    expect(formatG(2e-7)).toBe('2e-7');
    expect(formatG(0)).toBe('0');
    expect(formatG(123456)).toBe('123456');
  });
});

import { autoLengthUnit, formatSig } from './units';

describe('plot units', () => {
  it('picks the largest unit the span fills', () => {
    const m = (mm: number) => autoLengthUnit('metric', mm)[0];
    expect([0.001, 0.04, 0.999, 1, 1.28, 9.9, 10, 250, 1000, 5000].map(m)).toEqual(
      ['µm', 'µm', 'µm', 'mm', 'mm', 'mm', 'cm', 'cm', 'm', 'm'],
    );
    const i = (mm: number) => autoLengthUnit('imperial', mm)[0];
    expect([0.0005, 0.04, 1.28, 30, 400].map(i)).toEqual(['µin', 'mil', 'mil', 'in', 'ft']);
    expect(m(1e-9)).toBe('µm');
    expect(autoLengthUnit('metric', Number.NaN)).toEqual(['mm', 1]);
    expect(autoLengthUnit('imperial', -1)[0]).toBe('mil');
    expect(autoLengthUnit('imperial', 100)).toEqual(['in', 25.4]);
  });

  it('formats significant digits', () => {
    expect(formatSig(1.5, 4)).toBe('1.500');
    expect(formatSig(0.99999999, 4)).toBe('1.000');
    expect(formatSig(-59.0551, 5)).toBe('-59.055');
    expect(formatSig(0.0381, 4)).toBe('0.03810');
    expect(formatSig(12345.6, 4)).toBe('12346');
    expect(formatSig(0, 4)).toBe('0');
  });
});

import { fieldDbUnit, fieldPerVPerM, fieldPrefix, formatField } from './units';

describe('field units', () => {
  it('uses V/m and V/mil', () => {
    expect(fieldPerVPerM('imperial') * 39_370.07874).toBeCloseTo(1, 6);
    expect(fieldDbUnit('imperial')).toBe('dBV/mil');
  });

  it('picks the largest SI prefix that keeps the value at least 1', () => {
    for (const [v, want] of [[3e9, 'G'], [3e6, 'M'], [3e3, 'k'], [3, ''], [0.3, 'm'], [3e-4, 'µ'], [3e-9, 'µ']] as const) {
      expect(fieldPrefix(v)[0]).toBe(want);
    }
    expect(fieldPrefix(0)).toEqual(['', 1]);
    expect(fieldPrefix(Number.NaN)).toEqual(['', 1]);
    expect(fieldPrefix(999.99999999)[0]).toBe('k'); // rounds to 1.000 k
  });

  it('formats a field strength', () => {
    expect(formatField('metric', 15340)).toBe('15.34 kV/m');
    expect(formatField('imperial', 39_370.07874)).toBe('1.000 V/mil');
  });
});
