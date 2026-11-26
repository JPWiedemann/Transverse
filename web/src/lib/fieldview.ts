// View arithmetic of the field tab: a 1:1 view (centre and pixels per mm), when the image has to be
// rendered again, and the colour-bar scale.

import type { Legend } from './types';
import { fieldPrefix } from './units';

/** [x0, x1, y0, y1] in mm */
export type Bounds = [number, number, number, number];

/** Centre of the view [mm] and scale [pixels per mm]; x and y share the scale (1:1). */
export interface View {
  cx: number;
  cy: number;
  ppm: number;
}

/** The image is rendered this much larger than the visible region on every side. */
export const MARGIN = 0.25;
/** The image is rendered again when the zoom has changed by more than this (visible width / image's). */
export const RERENDER_RATIO: [number, number] = [0.6, 1.4];

export function expand(b: Bounds, frac: number): Bounds {
  const [dx, dy] = [(b[1] - b[0]) * frac, (b[3] - b[2]) * frac];
  return [b[0] - dx, b[1] + dx, b[2] - dy, b[3] + dy];
}

export function contains(outer: Bounds, inner: Bounds): boolean {
  return outer[0] <= inner[0] && outer[1] >= inner[1] && outer[2] <= inner[2] && outer[3] >= inner[3];
}

/** The view that shows `b` completely in a `w` x `h` pixel area. */
export function fitView(b: Bounds, w: number, h: number): View {
  const ppm = Math.min(w / (b[1] - b[0]), h / (b[3] - b[2]));
  return { cx: (b[0] + b[1]) / 2, cy: (b[2] + b[3]) / 2, ppm: Number.isFinite(ppm) && ppm > 0 ? ppm : 1 };
}

/** The region of the plane a view shows in a `w` x `h` pixel area. */
export function boundsOf(v: View, w: number, h: number): Bounds {
  const [hw, hh] = [w / 2 / v.ppm, h / 2 / v.ppm];
  return [v.cx - hw, v.cx + hw, v.cy - hh, v.cy + hh];
}

/** Zoom by `factor` (< 1 zooms in) keeping the plane point under the pixel (`px`, `py`) fixed. */
export function zoomAt(v: View, px: number, py: number, w: number, h: number, factor: number): View {
  const [x, y] = [v.cx + (px - w / 2) / v.ppm, v.cy - (py - h / 2) / v.ppm];
  const ppm = v.ppm / factor;
  return { ppm, cx: x - (px - w / 2) / ppm, cy: y + (py - h / 2) / ppm };
}

export function panBy(v: View, dxPx: number, dyPx: number): View {
  return { ...v, cx: v.cx - dxPx / v.ppm, cy: v.cy + dyPx / v.ppm };
}

/** What the current image was made for. */
export interface Rendered {
  key: string;
  /** visible region when it was rendered */
  visible: Bounds;
  /** region the image was asked to cover */
  covered: Bounds;
}

export function needsRender(r: Rendered | undefined, key: string, visible: Bounds): boolean {
  if (!r || r.key !== key || !contains(r.covered, visible)) return true;
  const ratio = (visible[1] - visible[0]) / (r.visible[1] - r.visible[0]);
  return !(ratio >= RERENDER_RATIO[0] && ratio <= RERENDER_RATIO[1]);
}

/** Colour bar title and the factor from the legend's values to the displayed ones: linear field
 * scales get an SI prefix chosen from the top of the scale (kV/m, MV/m, V/mil, ...), dB scales keep
 * their unit (dBV/m). */
export function colourbarScale(l: Legend): { title: string; mult: number } {
  if (l.field_base === null) return { title: l.label, mult: 1 };
  const [prefix, mult] = fieldPrefix(l.vmax);
  return { title: `|E| [${prefix}${l.field_base}]`, mult };
}
