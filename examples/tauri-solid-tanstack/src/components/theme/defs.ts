export interface ThemeDef {
  value: string;
  label: string;
  mode: 'dark' | 'light';
  monacoTheme: string; // file name in monaco-themes, or 'catppuccin-*' for built-in
}

export const THEMES: ThemeDef[] = [
  { value: 'catppuccin-macchiato', label: 'Catppuccin Macchiato', mode: 'dark',  monacoTheme: 'catppuccin-macchiato' },
  { value: 'catppuccin-latte',     label: 'Catppuccin Latte',     mode: 'light', monacoTheme: 'catppuccin-latte' },
  { value: 'dracula',              label: 'Dracula',              mode: 'dark',  monacoTheme: 'Dracula' },
  { value: 'github-dark',          label: 'GitHub Dark',          mode: 'dark',  monacoTheme: 'GitHub Dark' },
  { value: 'github-light',         label: 'GitHub Light',         mode: 'light', monacoTheme: 'GitHub Light' },
  { value: 'one-dark-pro',         label: 'One Dark Pro',         mode: 'dark',  monacoTheme: 'One Dark Pro' },
  { value: 'monokai',              label: 'Monokai',              mode: 'dark',  monacoTheme: 'Monokai' },
  { value: 'solarized-dark',       label: 'Solarized Dark',       mode: 'dark',  monacoTheme: 'Solarized-dark' },
  { value: 'solarized-light',      label: 'Solarized Light',      mode: 'light', monacoTheme: 'Solarized-light' },
  { value: 'night-owl',            label: 'Night Owl',            mode: 'dark',  monacoTheme: 'Night Owl' },
  { value: 'ayu-dark',             label: 'Ayu Dark',             mode: 'dark',  monacoTheme: 'Ayu-dark' },
  { value: 'ayu-light',            label: 'Ayu Light',            mode: 'light', monacoTheme: 'Ayu-light' },
  { value: 'nord',                 label: 'Nord',                 mode: 'dark',  monacoTheme: 'Nord' },
  { value: 'palenight',            label: 'Palenight',            mode: 'dark',  monacoTheme: 'Palenight' },
  { value: 'material-palenight',   label: 'Material Palenight',   mode: 'dark',  monacoTheme: 'Material Palenight' },
];

export function themeByName(value: string): ThemeDef | undefined {
  return THEMES.find(t => t.value === value);
}
