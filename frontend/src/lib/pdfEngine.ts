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
import { AnnotationPluginPackage } from '@embedpdf/plugin-annotation/svelte';
import { HistoryPluginPackage } from '@embedpdf/plugin-history';
import { type AnnotationKind, colorPatch, TOOL_BY_KIND } from './annotationAdapter';
import { DEFAULT_ANNOTATION_COLOR, colorHex } from './annotationPalette';

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
    createPluginRegistration(ZoomPluginPackage, { defaultZoomLevel: ZoomMode.FitPage }),
    // Visible-area high-res tiles; the full-page RenderLayer base stays at
    // scale 1 so zooming never re-renders whole pages (see PdfPages.svelte).
    // Defaults (tileSize 768) match the ready-made viewer; only pass config
    // here if a verified option needs changing.
    createPluginRegistration(TilingPluginPackage),
    // Toolbar features: find-in-document, page thumbnails, document outline.
    createPluginRegistration(SearchPluginPackage),
    // autoScroll would snap the pane to the current page on EVERY page
    // change, fighting manual thumbnail browsing (trackpad momentum keeps
    // firing page changes for a second after a flick). The side panel
    // positions the pane once on open instead, via a direct scrollTop write
    // (see PdfSidePanel.svelte) — scrollToThumb/scrollTo$ is never called,
    // so scrollBehavior never gets consulted. `scrollBehavior: 'auto'` is
    // kept anyway as spec-mandated defense-in-depth: nothing in this app
    // emits through that scroll channel anymore.
    createPluginRegistration(ThumbnailPluginPackage, { autoScroll: false, scrollBehavior: 'auto' }),
    createPluginRegistration(BookmarkPluginPackage),
    // Annotations are a SQLite sidecar, never a write to the PDF — see
    // ANNOTATION_OPTIONS. History is the annotation plugin's optional
    // dependency: registering it is all that's needed for undo/redo, which the
    // plugin files under the 'annotations' topic (ANNOTATION_HISTORY_TOPIC).
    createPluginRegistration(HistoryPluginPackage),
    createPluginRegistration(AnnotationPluginPackage, ANNOTATION_OPTIONS),
  ];
}

/// The topic the annotation plugin files its undo/redo commands under. Scoping
/// undo to it means the toolbar's undo can only ever take back a mark — never
/// whatever a future plugin puts on the same global timeline. The plugin keeps
/// this private (`ANNOTATION_HISTORY_TOPIC` in annotation-plugin.d.ts), so the
/// string is version-pinned to @embedpdf/plugin-annotation 2.14.4; a rename
/// upstream shows up as undo buttons that stay disabled, not as a wrong undo.
export const ANNOTATION_HISTORY_TOPIC = 'annotations';

/// Every tool we surface, seeded with the default palette color. The color is a
/// live preference, so the toolbar pushes changes through `setToolDefaults`
/// rather than this being the last word.
///
/// The plugin's own defaults are otherwise left alone. In particular a
/// highlight keeps `opacity: 1` with `blendMode: Multiply` — that multiply is
/// what lets the text show through, and swapping it for a translucent fill
/// washes the glyphs out instead.
function paletteTools(): { id: string; defaults: Record<string, string> }[] {
  const hex = colorHex(DEFAULT_ANNOTATION_COLOR);
  return Object.entries(TOOL_BY_KIND).map(([kind, id]) => ({
    id,
    defaults: colorPatch(kind as AnnotationKind, hex),
  }));
}

export const ANNOTATION_OPTIONS = {
  // The one setting this whole feature rests on. With autoCommit the plugin
  // would write marks back into the PDF through PDFium, changing the bytes
  // under `papers.content_hash` — the hash that drives ingest dedupe and the
  // `_unsorted/<hash>.pdf` name. Marks live in SQLite instead (see
  // src/annotations/), so the library file stays byte-identical forever.
  autoCommit: false,
  // The reader has its own citation popovers over link annotations
  // (CitationLayer). Letting the plugin open URIs on click would fire a
  // window.open behind that popover.
  autoOpenLinks: false,
  // Shown as the annotation's author in an exported copy. Deliberately not the
  // OS user: nothing here should leak a real name into a file the user shares.
  annotationAuthor: 'Xuewen',
  // A just-drawn mark is selected so its note editor and color swatches are one
  // click away, but the tool stays armed for marking several passages in a row.
  selectAfterCreate: true,
  deactivateToolAfterCreate: false,
  tools: paletteTools(),
} as const;
