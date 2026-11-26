// Per-viewer settings in the browser's localStorage (form values per line type, synthesis
// target, theme). Everything is optional: with storage blocked the page works without it.

import type { SavedForm } from './form';

export type Theme = 'system' | 'light' | 'dark';

export interface LineSettings {
  form?: SavedForm;
  synthTarget?: string;
  synthZ0?: string;
}

export interface Settings {
  theme: Theme;
  lines: Record<string, LineSettings>;
}

const KEY = 'transverse.settings.v1';

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const s = JSON.parse(raw) as Partial<Settings>;
      const theme = s.theme === 'light' || s.theme === 'dark' ? s.theme : 'system';
      return { theme, lines: typeof s.lines === 'object' && s.lines !== null ? s.lines : {} };
    }
  } catch {
    // unreadable or blocked: start with defaults
  }
  return { theme: 'system', lines: {} };
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
