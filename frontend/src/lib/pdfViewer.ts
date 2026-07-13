import type { ThemeMode } from './state.svelte';

/** EmbedPDF theme preference resolved from the app's theme mode. */
export function themePreference(mode: ThemeMode, systemDark: boolean): 'light' | 'dark' {
  if (mode === 'system') return systemDark ? 'dark' : 'light';
  return mode;
}

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
    wasmUrl: '/pdfium.wasm',
    worker: true,
    fonts: { ui: null, signature: null },
    fontFallback: null,
    stamp: { manifests: [] },
    tabBar: 'never',
    theme: { preference },
  };
}
