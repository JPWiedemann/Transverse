// The cheap, synchronous calls of the wasm module (form specs, wavelength) run on the page
// thread; only the solver runs in the worker. The module is small, so loading it twice is fine.

import init, { defaults, line_types, param_specs, schematic, wavelength_mm } from '../../pkg/transverse_wasm.js';
import type { LineInfo, ParamSet, ParamSpec, Schematic } from './types';

export async function loadMeta(): Promise<void> {
  await init();
}

export const lineTypes = (): LineInfo[] => JSON.parse(line_types()) as LineInfo[];
export const paramSpecs = (line: string): ParamSpec[] => JSON.parse(param_specs(line)) as ParamSpec[];
export const paramDefaults = (line: string): ParamSet => JSON.parse(defaults(line)) as ParamSet;

export function wavelengthOf(line: string): (set: ParamSet) => number | undefined {
  return (set) => {
    try {
      return wavelength_mm(line, JSON.stringify(set)) ?? undefined;
    } catch {
      return undefined;
    }
  };
}

/** Sketch of the cross-section for the given parameters (internal units), `null` if there is none. */
export const schematicOf = (line: string, set: ParamSet): Schematic | null =>
  JSON.parse(schematic(line, JSON.stringify(set))) as Schematic | null;
