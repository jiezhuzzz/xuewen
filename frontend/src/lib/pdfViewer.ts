import type { ThemeMode } from './state.svelte';

/** EmbedPDF theme preference resolved from the app's theme mode. */
export function themePreference(mode: ThemeMode, systemDark: boolean): 'light' | 'dark' {
  if (mode === 'system') return systemDark ? 'dark' : 'light';
  return mode;
}

// The app's palette (app.css @theme + Tailwind stone/amber) mapped onto
// EmbedPDF's ThemeColors tokens so the viewer's chrome matches the web UI:
// warm paper/soot surfaces, ink/stone text, and the amber-700/500 accent.
// Deep-merged over EmbedPDF's base theme, so only the tokens we set change.
const VIEWER_THEME = {
  light: {
    background: { app: '#f1efea', surface: '#faf9f7', surfaceAlt: '#f1efea', elevated: '#faf9f7', input: '#faf9f7' },
    foreground: { primary: '#1c1917', secondary: '#57534e', muted: '#78716c', disabled: '#a8a29e', onAccent: '#ffffff' },
    border: { default: '#e7e5e4', subtle: '#e7e5e4', strong: '#d6d3d1' },
    accent: { primary: '#b45309', primaryHover: '#92400e', primaryActive: '#78350f', primaryLight: '#fef3c7', primaryForeground: '#ffffff' },
    interactive: { hover: '#f1efea', active: '#e7e5e4', selected: '#f1efea', focus: '#b45309', focusRing: '#fde68a' },
  },
  dark: {
    background: { app: '#161311', surface: '#211d1a', surfaceAlt: '#161311', elevated: '#211d1a', input: '#211d1a' },
    foreground: { primary: '#f5f5f4', secondary: '#d6d3d1', muted: '#a8a29e', disabled: '#57534e', onAccent: '#1c1917' },
    border: { default: '#292524', subtle: '#292524', strong: '#44403c' },
    accent: { primary: '#f59e0b', primaryHover: '#fbbf24', primaryActive: '#d97706', primaryLight: '#451a03', primaryForeground: '#1c1917' },
    interactive: { hover: '#292524', active: '#44403c', selected: '#292524', focus: '#f59e0b', focusRing: '#78350f' },
  },
} as const;

/**
 * Offline, self-hosted `<PDFViewer>` config for one paper. Returns a plain
 * object (cast to the component's config at the call site) so this stays
 * hermetic and unit-testable without importing the EmbedPDF runtime.
 */
export function pdfViewerConfig(
  paperId: string,
  preference: 'light' | 'dark',
): Record<string, unknown> {
  return {
    src: `/papers/${encodeURIComponent(paperId)}/pdf`,
    // Load the self-hosted wasm, NOT EmbedPDF's default (the jsDelivr CDN) —
    // required for the app to work offline. Served from /pdfium.wasm by the
    // Task-1 build copy.
    wasmUrl: '/pdfium.wasm',
    // PDFium runs on the main thread, not a Web Worker. EmbedPDF's worker is a
    // `blob:` worker; in our (plain Vite, non-SvelteKit) production build it
    // never loads pdfium.wasm — the wasm URL resolves against the blob
    // `import.meta.url` and our `wasmUrl` isn't threaded into the worker's
    // Emscripten loader, so the viewer hangs on "Loading document...". The
    // main-thread `direct-engine` loads the self-hosted /pdfium.wasm correctly.
    // Fine for typical (few-MB) papers; revisit worker mode if large PDFs jank.
    worker: false,
    fonts: { ui: null, signature: null },
    fontFallback: null,
    stamp: { manifests: [] },
    tabBar: 'never',
    theme: { preference, light: VIEWER_THEME.light, dark: VIEWER_THEME.dark },
  };
}
