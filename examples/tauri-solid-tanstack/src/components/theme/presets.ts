export type ThemePreset = 'default' | 'catppuccin-macchiato' | 'catppuccin-latte';

const STORAGE_KEY = 'theme-preset';

const presets: Record<ThemePreset, { label: string }> = {
  'default': { label: 'Default' },
  'catppuccin-macchiato': { label: 'Catppuccin Macchiato' },
  'catppuccin-latte': { label: 'Catppuccin Latte' },
};

export function getThemePreset(): ThemePreset {
  if (typeof localStorage === 'undefined') return 'catppuccin-macchiato';
  return (localStorage.getItem(STORAGE_KEY) as ThemePreset) || 'catppuccin-macchiato';
}

export function setThemePreset(preset: ThemePreset): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(STORAGE_KEY, preset);
  applyPresetClass(preset);
}

export function applyPresetClass(preset: ThemePreset): void {
  const root = document.documentElement;
  for (const key of Object.keys(presets)) {
    root.classList.remove(`preset-${key}`);
  }
  if (preset !== 'default') {
    root.classList.add(`preset-${preset}`);
  }
}

export function getPresetOptions() {
  return Object.entries(presets).map(([value, { label }]) => ({ value, label }));
}
