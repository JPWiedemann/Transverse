// Editable state of a parameter form generated from the ParamSpecs: text fields with unit
// dropdowns, flags and choices. Text is kept as typed so half-finished input is never lost;
// conversion to internal units (mm, Hz) happens in `toParams`. Port of transverse-gui/form.rs.

import type { ParamSet, ParamSpec, Preset, Visibility } from './types';
import { FREQ_UNITS, LAMBDA, LEN_UNITS, factor as unitFactor, formatG, parseNumber } from './units';

export type FieldState =
  | { kind: 'quantity'; text: string; unit: string }
  | { kind: 'flag'; value: boolean }
  | { kind: 'choice'; index: number };

export type FormState = Record<string, FieldState>;

/** Returns the wavelength [mm] for a parameter set, if the line type has one. */
export type Wavelength = (set: ParamSet) => number | undefined;

export class FormError extends Error {
  constructor(
    readonly key: string,
    readonly label: string,
    message: string,
  ) {
    super(`${label}: ${message}`);
  }
}

export function unitOptions(spec: ParamSpec): string[] {
  const k = spec.kind;
  if ('Length' in k) return [...LEN_UNITS.map(([s]) => s), ...(k.Length.lambda ? [LAMBDA] : [])];
  if ('Frequency' in k) return FREQ_UNITS.map(([s]) => s);
  return [];
}

/** Initial display unit; the fixed unit label for plain numbers. */
export function defaultUnit(spec: ParamSpec): string {
  const k = spec.kind;
  if ('Length' in k) return k.Length.unit;
  if ('Frequency' in k) return k.Frequency.unit;
  if ('Number' in k) return k.Number.unit;
  return '';
}

/** Factor from the displayed unit to the internal one. */
function factor(spec: ParamSpec, unit: string, lambdaMm: number | undefined): number | undefined {
  const k = spec.kind;
  if ('Length' in k) return unitFactor(LEN_UNITS, unit, lambdaMm);
  if ('Frequency' in k) return unitFactor(FREQ_UNITS, unit, undefined);
  if ('Number' in k) return k.Number.scale;
  return undefined;
}

/** Show `set` in the fields, keeping each field's current display unit. */
export function setParams(
  specs: ParamSpec[],
  prev: FormState,
  set: ParamSet,
  wavelength: Wavelength,
): FormState {
  const lam = wavelength(set);
  const out: FormState = { ...prev };
  for (const spec of specs) {
    const v = set[spec.key];
    if (v === undefined) continue;
    const k = spec.kind;
    if ('Bool' in k && typeof v === 'boolean') {
      out[spec.key] = { kind: 'flag', value: v };
    } else if ('Choice' in k && typeof v === 'number') {
      out[spec.key] = { kind: 'choice', index: v };
    } else if (typeof v === 'number') {
      const old = prev[spec.key];
      let unit = old?.kind === 'quantity' ? old.unit : defaultUnit(spec);
      let f = factor(spec, unit, lam);
      if (f === undefined) {
        // fall back to the default unit if a λ reference is unavailable
        unit = defaultUnit(spec);
        f = factor(spec, unit, lam) ?? 1;
      }
      out[spec.key] = { kind: 'quantity', text: formatG(v / f), unit };
    }
  }
  return out;
}

/** Form with the defaults in their default display units. */
export function newForm(specs: ParamSpec[], defaults: ParamSet, wavelength: Wavelength): FormState {
  return setParams(specs, {}, defaults, wavelength);
}

/** Flags and choices only (everything else at its default): enough to decide which controls are
 * shown while some text field holds unfinished input. */
export function controlSet(defaults: ParamSet, form: FormState): ParamSet {
  const set: ParamSet = { ...defaults };
  for (const [k, f] of Object.entries(form)) {
    if (f.kind === 'flag') set[k] = f.value;
    else if (f.kind === 'choice') set[k] = f.index;
    // visibility conditions on a quantity only ask for zero, which is zero in every unit
    else if (parseNumber(f.text) === 0) set[k] = 0;
  }
  return set;
}

/** Whether something with this visibility applies to the values. */
export function isVisible(when: Visibility, set: ParamSet): boolean {
  const holds = (c: { key: string; value: number; not?: boolean }) => {
    const v = set[c.key];
    if (v === undefined) return true;
    return (Number(v) === c.value) !== (c.not ?? false);
  };
  return when.length === 0 || when.some((all) => all.every(holds));
}

/** The form with the switches and choices of a preset; all other fields stay as they are. */
export function applyPreset(form: FormState, preset: Preset): FormState {
  const out: FormState = { ...form };
  for (const [key, value] of preset.values) {
    const cur = out[key];
    if (cur?.kind === 'flag' && typeof value === 'boolean') out[key] = { kind: 'flag', value };
    else if (cur?.kind === 'choice' && typeof value === 'number') out[key] = { kind: 'choice', index: value };
  }
  return out;
}

/** Parameters in internal units. Sizes given in λ refer to the wavelength computed from the
 * other fields. Throws a FormError for the first invalid field. A hidden field cannot hold anything up:
 * if its text is no number, it keeps its default (and its text). */
export function toParams(
  specs: ParamSpec[],
  defaults: ParamSet,
  form: FormState,
  wavelength: Wavelength,
): ParamSet {
  const set: ParamSet = { ...defaults };
  const controls = controlSet(defaults, form);
  for (const pass of [false, true]) {
    const lam = pass ? wavelength(set) : undefined;
    for (const spec of specs) {
      const state = form[spec.key];
      if (!state) continue;
      if (state.kind === 'flag') set[spec.key] = state.value;
      else if (state.kind === 'choice') set[spec.key] = state.index;
      else {
        if ((state.unit === LAMBDA) !== pass) continue;
        const v = parseNumber(state.text);
        const f = factor(spec, state.unit, lam);
        if (v !== undefined && f !== undefined) set[spec.key] = v * f;
        else if (isVisible(spec.visible_when, controls)) {
          const why = v === undefined ? `'${state.text.trim()}' is not a number` : 'no wavelength reference';
          throw new FormError(spec.key, spec.label, why);
        }
      }
    }
  }
  return set;
}

/** Switch the display unit of a field and convert the shown value (an unparsable text is kept). */
export function changeUnit(
  specs: ParamSpec[],
  defaults: ParamSet,
  form: FormState,
  key: string,
  newUnit: string,
  wavelength: Wavelength,
): FormState {
  const spec = specs.find((s) => s.key === key);
  const st = form[key];
  if (!spec || st?.kind !== 'quantity' || !unitOptions(spec).includes(newUnit)) return form;
  let lam: number | undefined;
  try {
    lam = wavelength(toParams(specs, defaults, form, wavelength));
  } catch {
    lam = undefined;
  }
  const v = parseNumber(st.text);
  const fo = factor(spec, st.unit, lam);
  const fn = factor(spec, newUnit, lam);
  const text = v !== undefined && fo !== undefined && fn !== undefined ? formatG((v * fo) / fn) : st.text;
  return { ...form, [key]: { kind: 'quantity', text, unit: newUnit } };
}

/** Saved form (same idea as the settings file of the other frontends: text as typed plus the unit). */
export type SavedForm = Record<string, { value?: string; unit?: string; flag?: boolean; choice?: number }>;

export function toSaved(form: FormState): SavedForm {
  const out: SavedForm = {};
  for (const [k, f] of Object.entries(form)) {
    out[k] =
      f.kind === 'quantity'
        ? { value: f.text, ...(f.unit ? { unit: f.unit } : {}) }
        : f.kind === 'flag'
          ? { flag: f.value }
          : { choice: f.index };
  }
  return out;
}

/** Restore saved fields; unknown keys, wrong kinds and invalid units are ignored. */
export function loadSaved(specs: ParamSpec[], form: FormState, saved: unknown): FormState {
  if (typeof saved !== 'object' || saved === null) return form;
  const s = saved as SavedForm;
  const out: FormState = { ...form };
  for (const spec of specs) {
    const sv = s[spec.key];
    const cur = out[spec.key];
    if (!sv || typeof sv !== 'object' || !cur) continue;
    if (cur.kind === 'flag' && typeof sv.flag === 'boolean') out[spec.key] = { kind: 'flag', value: sv.flag };
    else if (cur.kind === 'choice') {
      const k = spec.kind;
      const n = 'Choice' in k ? k.Choice[0].length : 0;
      if (Number.isInteger(sv.choice) && (sv.choice as number) >= 0 && (sv.choice as number) < n)
        out[spec.key] = { kind: 'choice', index: sv.choice as number };
    } else if (cur.kind === 'quantity') {
      const fixed = 'Number' in spec.kind;
      const unit =
        !fixed && typeof sv.unit === 'string' && unitOptions(spec).includes(sv.unit) ? sv.unit : cur.unit;
      const text = typeof sv.value === 'string' ? sv.value : cur.text;
      out[spec.key] = { kind: 'quantity', text, unit };
    }
  }
  return out;
}
