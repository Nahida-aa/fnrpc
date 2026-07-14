import {
	createContext,
	createEffect,
	createMemo,
	createSignal,
	type JSX,
	onCleanup,
	onMount,
	useContext,
} from 'solid-js';
import { isServer } from 'solid-js/web';
import { THEMES, themeByName } from './defs';
import type { ThemeDef } from './defs';

const STORAGE_KEY_SCHEME = 'color-scheme';
const STORAGE_KEY_THEME = 'theme-name';

export type ColorScheme = 'system' | 'light' | 'dark';
type ActiveTheme = 'dark' | 'light';

const getSaved = (key: string, fallback: string) => {
	if (typeof localStorage === 'undefined') return fallback;
	return localStorage.getItem(key) || fallback;
};

export const themeScript = `(()=>{
var cs=localStorage.getItem('${STORAGE_KEY_SCHEME}')||'system';
var tn=localStorage.getItem('${STORAGE_KEY_THEME}')||'catppuccin-macchiato';
var themes=${JSON.stringify(THEMES.map(t => ({ value: t.value, mode: t.mode })))};
var found=themes.find(function(x){return x.value===tn});
var mode=found?found.mode:'dark';
if(cs==='system'){mode=matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light'}
else if(cs==='light'){mode='light'}
else if(cs==='dark'){mode='dark'}
document.documentElement.classList.add(mode)
})()`;

type ThemeProviderState = {
	colorScheme: () => ColorScheme;
	setColorScheme: (cs: ColorScheme) => void;
	systemTheme: () => 'light' | 'dark';
	activeTheme: () => ActiveTheme;
	themeName: () => string;
	setThemeName: (name: string) => void;
	currentThemeDef: () => ThemeDef | undefined;
};

const ThemeProviderContext = createContext<ThemeProviderState>();

const getSystemTheme = () =>
	typeof window !== 'undefined' ?
	window.matchMedia('(prefers-color-scheme: dark)').matches
		? 'dark' as const
		: 'light' as const
	: 'light' as const;

export function ThemeProvider(props: { children: JSX.Element }) {
	const [colorScheme, setColorScheme] = createSignal<ColorScheme>(
		getSaved(STORAGE_KEY_SCHEME, 'system') as ColorScheme,
	);
	const [themeName, setThemeName] = createSignal(
		getSaved(STORAGE_KEY_THEME, 'catppuccin-macchiato'),
	);
	const [systemTheme, setSystemTheme] = createSignal(getSystemTheme());
	const [mounted, setMounted] = createSignal(!isServer);

	const currentThemeDef = createMemo(() => themeByName(themeName()));

	const resolvedMode = createMemo(() => {
		const cs = colorScheme();
		if (cs === 'system') return systemTheme();
		if (cs === 'light') return 'light';
		return 'dark';
	});

	onMount(() => {
		setMounted(true);
		const root = document.documentElement;
		root.classList.remove('light', 'dark');
		root.classList.add(resolvedMode());

		const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
		setSystemTheme(mediaQuery.matches ? 'dark' : 'light');
		const handleChange = () => setSystemTheme(mediaQuery.matches ? 'dark' : 'light');
		mediaQuery.addEventListener('change', handleChange);
		onCleanup(() => mediaQuery.removeEventListener('change', handleChange));
	});

	createEffect(() => {
		if (!mounted()) return;
		const root = window.document.documentElement;
		const mode = resolvedMode();
		root.classList.remove('light', 'dark');
		root.classList.add(mode);
	});

	const value = {
		colorScheme,
		setColorScheme: (cs: ColorScheme) => {
			localStorage.setItem(STORAGE_KEY_SCHEME, cs);
			setColorScheme(cs);
		},
		systemTheme,
		activeTheme: resolvedMode,
		themeName,
		setThemeName: (name: string) => {
			localStorage.setItem(STORAGE_KEY_THEME, name);
			setThemeName(name);
		},
		currentThemeDef,
	};

	return (
		<ThemeProviderContext.Provider value={value}>
			{props.children}
		</ThemeProviderContext.Provider>
	);
}

export const useTheme = () => {
	const context = useContext(ThemeProviderContext);
	if (!context) throw new Error('useTheme must be used within a ThemeProvider');
	return context;
};
