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
