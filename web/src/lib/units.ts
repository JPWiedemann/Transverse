// Display units. Internally everything is mm and Hz (same tables as transverse-core/units.rs).

/** [symbol, factor to mm] */
export const LEN_UNITS: [string, number][] = [
  ['µm', 1e-3],
  ['mm', 1],
  ['cm', 10],
  ['m', 1000],
  ['mil', 0.0254],
  ['in', 25.4],
  ['ft', 304.8],
];

/** [symbol, factor to Hz] */
export const FREQ_UNITS: [string, number][] = [
  ['Hz', 1],
  ['kHz', 1e3],
  ['MHz', 1e6],
  ['GHz', 1e9],
];

/** Mesh sizes can be given relative to the wavelength. */
export const LAMBDA = 'λ';

/** Factor of a unit symbol; `lambdaMm` is the wavelength for the λ unit. */
export function factor(
  table: [string, number][],
  unit: string,
  lambdaMm: number | undefined,
): number | undefined {
  if (unit === LAMBDA) return lambdaMm !== undefined && Number.isFinite(lambdaMm) && lambdaMm > 0 ? lambdaMm : undefined;
  return table.find(([s]) => s === unit)?.[1];
}

/** Parse user input: decimal point or comma, surrounding spaces ignored. */
export function parseNumber(text: string): number | undefined {
  const t = text.trim().replace(',', '.');
  if (t === '') return undefined;
  const v = Number(t);
  return Number.isFinite(v) ? v : undefined;
}

/** Like Python's `{:.6g}`: six significant digits, no trailing zeros. */
export function formatG(v: number): string {
  if (v === 0 || !Number.isFinite(v)) return String(v);
  const exp = Math.floor(Math.log10(Math.abs(v)));
  const strip = (s: string) => (s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s);
  if (exp < -5 || exp >= 6) {
    const [m = '', e = '0'] = v.toExponential(5).split('e');
    return `${strip(m)}e${parseInt(e, 10)}`;
  }
  return strip(v.toFixed(Math.max(5 - exp, 0)));
}

/** `v` with `digits` significant digits (fixed notation, no exponent; large values keep all integer digits). */
export function formatSig(v: number, digits: number): string {
  if (v === 0 || !Number.isFinite(v)) return String(v);
  if (Math.abs(v) >= 10 ** (digits - 1)) return v.toFixed(0);
  // round to the significant digits first: 0.99999999 must read "1.000", not "1.0000"
  const r = Number(v.toExponential(digits - 1));
  const decimals = Math.min(Math.max(digits - 1 - Math.floor(Math.log10(Math.abs(r))), 0), 12);
  return r.toFixed(decimals);
}

/** Unit system for lengths shown on plots. */
export type UnitSystem = 'metric' | 'imperial';

/** [symbol, factor to mm] the automatic choice picks from, smallest first. */
export function autoUnits(system: UnitSystem): [string, number][] {
  return system === 'metric'
    ? [['µm', 1e-3], ['mm', 1], ['cm', 10], ['m', 1000]]
    : [['µin', 2.54e-5], ['mil', 0.0254], ['in', 25.4], ['ft', 304.8]];
}

/** The unit that suits a quantity of size `spanMm`: the largest one in which the span is at least 1.
 * Falls back to the smallest unit for tiny spans and to mm / in for invalid ones. */
export function autoLengthUnit(system: UnitSystem, spanMm: number): [string, number] {
  const units = autoUnits(system);
  if (!Number.isFinite(spanMm) || spanMm <= 0) return units[1]!;
  return [...units].reverse().find(([, f]) => spanMm / f >= 1) ?? units[0]!;
}

/** Value in the field unit of the unit system per V/m (1 V/mil = 39 370 V/m). */
export const fieldPerVPerM = (system: UnitSystem): number => (system === 'metric' ? 1 : 0.0254e-3);
export const fieldUnit = (system: UnitSystem): string => (system === 'metric' ? 'V/m' : 'V/mil');
export const fieldDbUnit = (system: UnitSystem): string => (system === 'metric' ? 'dBV/m' : 'dBV/mil');

/** SI prefix for a field strength given in the base unit: the largest prefix in which the value is
 * at least 1 (µ ... G). Returns [prefix, factor from the base unit to the prefixed unit].
 * Zero and invalid values keep the base unit. */
export function fieldPrefix(valueInBase: number): [string, number] {
  const prefixes: [string, number][] = [['G', 1e-9], ['M', 1e-6], ['k', 1e-3], ['', 1], ['m', 1e3], ['µ', 1e6]];
  const v = Math.abs(valueInBase);
  if (!Number.isFinite(v) || v === 0) return ['', 1];
  return prefixes.find(([, m]) => v * m >= 1 - 1e-9) ?? ['µ', 1e6];
}

/** "15.34 kV/m" for a field strength given in V/m, in the unit system's base unit with a suitable prefix. */
export function formatField(system: UnitSystem, eVPerM: number): string {
  const base = eVPerM * fieldPerVPerM(system);
  const [prefix, mult] = fieldPrefix(base);
  return `${formatSig(base * mult, 4)} ${prefix}${fieldUnit(system)}`;
}
