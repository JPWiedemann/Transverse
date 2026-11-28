// Per-viewer settings in the browser's localStorage (form values per line type, synthesis
// target, theme). Everything is optional: with storage blocked the page works without it.

import type { SavedForm } from './form';
import type { UnitSystem } from './loss';

export type Theme = 'system' | 'light' | 'dark';
export type FieldMode = 'e_db_lines' | 'e_db' | 'e_linear' | 'potential' | 'permittivity' | 'mesh';
export const FIELD_MODES: FieldMode[] = ['e_db_lines', 'e_db', 'e_linear', 'potential', 'permittivity', 'mesh'];

export interface LineSettings {
  form?: SavedForm;
  synthTarget?: string;
  synthZ0?: string;
  /** the second target of a search in two dimensions, as typed */
  synthSecond?: string;
  /** electrical length to reach in degrees, as typed; empty keeps the line length */
  synthPhase?: string;
}

export interface Settings {
  theme: Theme;
  units: UnitSystem;
  lossFmaxText: string;
  lossFmaxUnit: string;
  fieldMode: FieldMode;
  showEq: boolean;
  showMesh: boolean;
  dbRange: number;
  lines: Record<string, LineSettings>;
}

const KEY = 'transverse.settings.v1';

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const s = JSON.parse(raw) as Partial<Settings>;
      const theme = s.theme === 'light' || s.theme === 'dark' ? s.theme : 'system';
      return {
        ...defaults(),
        theme,
        units: s.units === 'imperial' ? 'imperial' : 'metric',
        lossFmaxText: typeof s.lossFmaxText === 'string' ? s.lossFmaxText : '20',
        lossFmaxUnit: typeof s.lossFmaxUnit === 'string' ? s.lossFmaxUnit : 'GHz',
        fieldMode: FIELD_MODES.find((m) => m === s.fieldMode) ?? 'e_db_lines',
        showEq: typeof s.showEq === 'boolean' ? s.showEq : true,
        showMesh: typeof s.showMesh === 'boolean' ? s.showMesh : false,
        dbRange: typeof s.dbRange === 'number' && s.dbRange >= 5 && s.dbRange <= 200 ? s.dbRange : 40,
        lines: typeof s.lines === 'object' && s.lines !== null ? migrateLines(s.lines) : {},
      };
    }
  } catch {
    // unreadable or blocked: start with defaults
  }
  return defaults();
}

/** Ids that line types had before: settings saved under them are read under the current id. */
const RENAMED_LINES: [string, string][] = [['cpwg_inner', 'planar']];

export function migrateLines(lines: Record<string, LineSettings>): Record<string, LineSettings> {
  const out = { ...lines };
  for (const [old, now] of RENAMED_LINES) {
    const saved = out[old];
    if (saved) {
      out[now] ??= saved;
      delete out[old];
    }
  }
  return out;
}

function defaults(): Settings {
  return { theme: 'system', units: 'metric', lossFmaxText: '20', lossFmaxUnit: 'GHz', fieldMode: 'e_db_lines', showEq: true, showMesh: false, dbRange: 40, lines: {} };
}

export function saveSettings(s: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // storage full or blocked: ignore
  }
}

export function applyTheme(theme: Theme): void {
  if (theme === 'system') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', theme);
}
