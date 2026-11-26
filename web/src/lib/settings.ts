// Per-viewer settings in the browser's localStorage (form values per line type, synthesis
// target, theme). Everything is optional: with storage blocked the page works without it.

import type { SavedForm } from './form';
import type { UnitSystem } from './loss';

export type Theme = 'system' | 'light' | 'dark';
export type Tab = 'results' | 'cross-section' | 'return-current' | 'loss';

export interface LineSettings {
  form?: SavedForm;
  synthTarget?: string;
  synthZ0?: string;
}

export interface Settings {
  theme: Theme;
  units: UnitSystem;
  tab: Tab;
  lossFmaxText: string;
  lossFmaxUnit: string;
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
        tab: s.tab === 'cross-section' || s.tab === 'return-current' || s.tab === 'loss' ? s.tab : 'results',
        lossFmaxText: typeof s.lossFmaxText === 'string' ? s.lossFmaxText : '20',
        lossFmaxUnit: typeof s.lossFmaxUnit === 'string' ? s.lossFmaxUnit : 'GHz',
        lines: typeof s.lines === 'object' && s.lines !== null ? s.lines : {},
      };
    }
  } catch {
    // unreadable or blocked: start with defaults
  }
  return defaults();
}

function defaults(): Settings {
  return { theme: 'system', units: 'metric', tab: 'results', lossFmaxText: '20', lossFmaxUnit: 'GHz', lines: {} };
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
