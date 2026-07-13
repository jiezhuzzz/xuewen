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
    theme: { preference },
  };
}
