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
