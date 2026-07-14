import { createPluginRegistration } from '@embedpdf/core';
import type { PluginBatchRegistrations } from '@embedpdf/core';
import { ViewportPluginPackage } from '@embedpdf/plugin-viewport';
import { ScrollPluginPackage } from '@embedpdf/plugin-scroll';
import { RenderPluginPackage } from '@embedpdf/plugin-render';
import { SelectionPluginPackage } from '@embedpdf/plugin-selection';
import { InteractionManagerPluginPackage } from '@embedpdf/plugin-interaction-manager';
import { DocumentManagerPluginPackage } from '@embedpdf/plugin-document-manager';
import { ZoomPluginPackage, ZoomMode } from '@embedpdf/plugin-zoom';
import { TilingPluginPackage } from '@embedpdf/plugin-tiling';
import { SearchPluginPackage } from '@embedpdf/plugin-search';
import { ThumbnailPluginPackage } from '@embedpdf/plugin-thumbnail';
import { BookmarkPluginPackage } from '@embedpdf/plugin-bookmark';

// Load-bearing offline config (see CLAUDE.md "PDF viewer gotchas"):
//  - worker:true   -> PDFium runs in EmbedPDF's stock blob module worker. The
//    worker's self.location is a blob: URL, which cannot resolve a
//    path-absolute fetch like '/pdfium.wasm' (Chromium throws "Failed to
//    parse URL from /pdfium.wasm" — there's no hierarchical path on a blob:
//    base to graft it onto). Passing a fully-qualified URL sidesteps that
//    entirely, since it needs no base-relative resolution.
//  - wasmUrl       -> self-hosted, resolved to an absolute same-origin URL
//    (default is a jsDelivr CDN, which breaks offline)
//  - fontFallback:null -> no external font fetches
export const ENGINE_OPTIONS = {
  wasmUrl: new URL('/pdfium.wasm', location.origin).href,
  worker: true,
  fontFallback: null,
} as const;

// One shared registry hosts every open paper as a document (EmbedPDF's Svelte
// bindings use a module-level singleton context, so there can only be ONE
// <EmbedPDF> per page). `maxDocuments` caps how many tabs can be open at once.
const MAX_OPEN_DOCUMENTS = 32;

/**
 * Plugin registrations for the single, app-level viewer. Documents are opened
 * per tab at runtime via the document-manager capability (no `initialDocuments`).
 */
export function viewerPlugins(): PluginBatchRegistrations {
  return [
    createPluginRegistration(DocumentManagerPluginPackage, {
      maxDocuments: MAX_OPEN_DOCUMENTS,
    }),
    createPluginRegistration(ViewportPluginPackage),
    createPluginRegistration(ScrollPluginPackage),
    createPluginRegistration(RenderPluginPackage),
    createPluginRegistration(InteractionManagerPluginPackage),
    createPluginRegistration(SelectionPluginPackage),
    createPluginRegistration(ZoomPluginPackage, { defaultZoomLevel: ZoomMode.FitWidth }),
    // Visible-area high-res tiles; the full-page RenderLayer base stays at
    // scale 1 so zooming never re-renders whole pages (see PdfPages.svelte).
    // Defaults (tileSize 768) match the ready-made viewer; only pass config
    // here if a verified option needs changing.
    createPluginRegistration(TilingPluginPackage),
    // Toolbar features: find-in-document, page thumbnails, document outline.
    createPluginRegistration(SearchPluginPackage),
    createPluginRegistration(ThumbnailPluginPackage),
    createPluginRegistration(BookmarkPluginPackage),
  ];
}
