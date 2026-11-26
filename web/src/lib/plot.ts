// Small helpers for the SVG plots.

/** Smallest 1, 2 or 5 times a power of ten that is >= x (x > 0). */
export function niceStep(x: number): number {
  const mag = 10 ** Math.floor(Math.log10(x));
  return ([1, 2, 5, 10].map((m) => m * mag).find((s) => s >= x * (1 - 1e-12)) ?? 10 * mag);
}

/** Round tick values inside [min, max], about `approx` of them. */
export function ticks(min: number, max: number, approx = 6): number[] {
  if (!(max > min) || !Number.isFinite(min) || !Number.isFinite(max)) return [];
  const step = niceStep((max - min) / approx);
  const out: number[] = [];
  for (let k = Math.ceil(min / step - 1e-9); k * step <= max + step * 1e-9; k++) {
    out.push(Number((k * step).toPrecision(12)));
  }
  return out;
}

/** Zoom the range [lo, hi] by `factor` (< 1 zooms in) around the value `at`. */
export function zoomRange(lo: number, hi: number, at: number, factor: number): [number, number] {
  return [at - (at - lo) * factor, at + (hi - at) * factor];
}
