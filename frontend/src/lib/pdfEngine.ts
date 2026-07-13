import { createPluginRegistration } from '@embedpdf/core';
import type { PluginBatchRegistrations } from '@embedpdf/core';
import { ViewportPluginPackage } from '@embedpdf/plugin-viewport';
import { ScrollPluginPackage } from '@embedpdf/plugin-scroll';
import { RenderPluginPackage } from '@embedpdf/plugin-render';
import { SelectionPluginPackage } from '@embedpdf/plugin-selection';
import { InteractionManagerPluginPackage } from '@embedpdf/plugin-interaction-manager';
import { DocumentManagerPluginPackage } from '@embedpdf/plugin-document-manager';
import { ZoomPluginPackage, ZoomMode } from '@embedpdf/plugin-zoom';
import type { ThemeMode } from './state.svelte';

/** EmbedPDF theme preference resolved from the app's theme mode. */
export function themePreference(mode: ThemeMode, systemDark: boolean): 'light' | 'dark' {
  if (mode === 'system') return systemDark ? 'dark' : 'light';
  return mode;
}

// Load-bearing offline config (see CLAUDE.md "PDF viewer gotchas"):
//  - worker:false  -> PDFium on the main thread (the blob worker never loads
//    our self-hosted wasm in this plain-Vite build; default hangs on "Loading…")
//  - wasmUrl       -> self-hosted /pdfium.wasm (default is a jsDelivr CDN, breaks offline)
//  - fontFallback:null -> no external font fetches
export const ENGINE_OPTIONS = {
  wasmUrl: '/pdfium.wasm',
  worker: false,
  fontFallback: null,
} as const;

/**
 * Plugin registrations for one paper's viewer. `pdfHref` is the same-origin
 * URL the backend serves the PDF from (`/papers/{id}/pdf`).
 */
export function viewerPlugins(pdfHref: string): PluginBatchRegistrations {
  return [
    createPluginRegistration(DocumentManagerPluginPackage, {
      initialDocuments: [{ url: pdfHref }],
    }),
    createPluginRegistration(ViewportPluginPackage),
    createPluginRegistration(ScrollPluginPackage),
    createPluginRegistration(RenderPluginPackage),
    createPluginRegistration(InteractionManagerPluginPackage),
    createPluginRegistration(SelectionPluginPackage),
    createPluginRegistration(ZoomPluginPackage, { defaultZoomLevel: ZoomMode.FitWidth }),
  ];
}
