<script lang="ts">
  import { onDestroy } from 'svelte';
  import { useRegistry, useDocumentState } from '@embedpdf/core/svelte';
  import { Viewport } from '@embedpdf/plugin-viewport/svelte';
  import { Scroller, type PageLayout } from '@embedpdf/plugin-scroll/svelte';
  import { ZoomGestureWrapper } from '@embedpdf/plugin-zoom/svelte';
  import { DocumentContent } from '@embedpdf/plugin-document-manager/svelte';
  import { RenderLayer } from '@embedpdf/plugin-render/svelte';
  import { SelectionLayer } from '@embedpdf/plugin-selection/svelte';
  import { PagePointerProvider } from '@embedpdf/plugin-interaction-manager/svelte';
  import { TilingLayer } from '@embedpdf/plugin-tiling/svelte';
  import PdfToolbar from './PdfToolbar.svelte';
  import PdfQuickActions from './PdfQuickActions.svelte';
  import PdfFindBar from './PdfFindBar.svelte';
  import PdfSidePanel from './PdfSidePanel.svelte';
  import { SearchLayer } from '@embedpdf/plugin-search/svelte';
  import CitationLayer from './CitationLayer.svelte';
  import { loadCitations, type EngineLike } from '../lib/loadCitations';
  import { libraryTitleIndex, matchReferences } from '../lib/citationMatch';
  import { parseCitations } from '../lib/api';
  import { runWhenIdle } from '../lib/idle';
  import { mergeStructured } from '../lib/refMerge';
  import { resolveAuthorYearMarkers } from '../lib/textCitations';
  import { reader } from '../lib/readerState.svelte';
  import { createPillHide } from '../lib/pillHide.svelte';
  import type { CitationData } from '../lib/citations';
  import type { PaperSummary } from '../lib/types';

  // Renders one paper's pages inside the shared <EmbedPDF> (see PdfViewer/PdfDeck).
  // Bound to its own `documentId` — one PdfPages is mounted per open tab — so the
  // shared engine is fine while each tab reads/extracts its own document.
  let { documentId }: { documentId: string } = $props();

  const ctx = useRegistry();
  const docState = useDocumentState(() => documentId);

  // Shared zen auto-hide for the floating pills (see lib/pillHide.svelte.ts).
  const pill = createPillHide(() => documentId);
  let pillHost = $state<HTMLDivElement | undefined>();
  $effect(() => {
    pill.setHost(pillHost ?? null);
  });

  let citations = $state<CitationData>({ references: [], markers: [] });
  let matches = $state<Map<number, PaperSummary>>(new Map());
  let pageSizes = $state<{ width: number; height: number }[]>([]);

  // Extract citation markers + match them against the library ONCE per document.
  // `docState.current` (and its `.document`) is reassigned on any core change —
  // incl. zoom scale and an initial load→reload — so guarding on the document
  // object's identity still re-ran extraction. Guard on the (fixed) documentId
  // instead: one PdfPages is mounted per tab, so extraction runs exactly once.
  // Extraction is scheduled once per document at idle; the schedule itself is
  // a true one-shot, cancelled ONLY on component unmount. This $effect can
  // legitimately re-run on the same document (zoom/reload churn re-fires
  // `docState.current`) — such re-runs must NOT cancel a pending/in-flight
  // extraction, so this effect returns no cleanup. Failures are caught/logged
  // so the reader still works without citation hovers.
  let extractedFor: string | null = null;
  let extractionCancelled = false;
  let cancelExtractionIdle: (() => void) | null = null;
  $effect(() => {
    const registry = ctx.registry;
    const rawDoc = docState.current?.document ?? null;
    if (!registry || !rawDoc || extractedFor === documentId) return;
    extractedFor = documentId;
    pageSizes = rawDoc.pages.map((p) => ({ width: p.size.width, height: p.size.height }));
    const engine = registry.getEngine();
    // The document/page objects are Svelte $state proxies (EmbedPDF's core
    // wraps them for reactivity). PDFium now runs in a worker (see
    // pdfEngine.ts), and every engine call round-trips doc/page through
    // postMessage — a live Proxy throws DataCloneError there. Snapshot once
    // into plain data so loadCitations can hand it back to the worker.
    const doc = $state.snapshot(rawDoc);
    // Extraction is PDFium work (now off the main thread, in the worker) —
    // wait for idle so the first pages paint before we start crawling
    // annotations/text.
    cancelExtractionIdle = runWhenIdle(() => {
      void (async () => {
        try {
          const data = await loadCitations(engine as unknown as EngineLike, doc);
          if (extractionCancelled) return;
          citations = data;
          // Whole-library title index, independent of the current UI filter
          // and shared across all open tabs (one fetch + normalization pass).
          const index = await libraryTitleIndex();
          if (extractionCancelled) return;
          matches = matchReferences(data.references, index);

          let refs = data.references;
          if (refs.length > 0) {
            // Structured upgrade — one POST per open; failure keeps raw text.
            const structured = await parseCitations(documentId, refs.map((r) => r.rawText));
            if (extractionCancelled) return;
            if (structured) refs = mergeStructured(refs, structured);
          }
          // Fallback author-year markers resolve best with structured entries,
          // and degrade to raw entry heads when the parse is unavailable.
          const extra = data.pendingAuthorYear?.length
            ? resolveAuthorYearMarkers(data.pendingAuthorYear, refs)
            : [];
          citations = { references: refs, markers: [...data.markers, ...extra] };
          matches = matchReferences(refs, index);
        } catch (err) {
          console.warn('citation extraction failed', err); // reader still works
        }
      })();
    });
  });

  onDestroy(() => {
    extractionCancelled = true;
    cancelExtractionIdle?.();
  });
</script>

{#snippet renderPage(page: PageLayout)}
  <div style:width="{page.width}px" style:height="{page.height}px" style:position="relative">
    <PagePointerProvider {documentId} pageIndex={page.pageIndex}>
      <!-- Low-res base rendered once (scale locked at 1, CSS-scaled by the
           framework); TilingLayer draws crisp visible tiles at the real zoom.
           This mirrors the ready-made viewer and is the perf fix — do NOT
           remove scale={1} or pages re-render fully on every zoom. -->
      <RenderLayer {documentId} pageIndex={page.pageIndex} scale={1} class="pointer-events-none" />
      <TilingLayer {documentId} pageIndex={page.pageIndex} class="pointer-events-none" />
      <SelectionLayer {documentId} pageIndex={page.pageIndex} />
      <SearchLayer
        {documentId}
        pageIndex={page.pageIndex}
        class="pointer-events-none"
        highlightColor="rgba(180, 83, 9, 0.28)"
        activeHighlightColor="rgba(180, 83, 9, 0.55)"
      />
      <CitationLayer
        pageIndex={page.pageIndex}
        pageWidthPt={pageSizes[page.pageIndex]?.width ?? page.width}
        pageHeightPt={pageSizes[page.pageIndex]?.height ?? page.height}
        data={citations}
        {matches}
      />
    </PagePointerProvider>
  </div>
{/snippet}

<svelte:window onpointermove={(e) => pill.onWindowMove(e)} />

<DocumentContent {documentId}>
  {#snippet children(doc)}
    {#if doc.isLoaded}
      <div class="flex h-full">
        {#if reader.panel[documentId]}
          <PdfSidePanel {documentId} />
        {/if}
        <div class="relative min-w-0 flex-1" bind:this={pillHost}>
          <PdfToolbar {documentId} {pill} />
          <PdfQuickActions {pill} />
          {#if reader.find[documentId]}
            <PdfFindBar {documentId} />
          {/if}
          <Viewport {documentId} class="h-full w-full">
            <ZoomGestureWrapper {documentId} class="h-full w-full">
              <Scroller {documentId} {renderPage} />
            </ZoomGestureWrapper>
          </Viewport>
        </div>
      </div>
    {:else if doc.isError}
      <p class="p-4 text-sm text-red-600 dark:text-red-400">Failed to load document.</p>
    {:else}
      <p class="p-4 text-sm text-stone-500 dark:text-stone-400">Loading document…</p>
    {/if}
  {/snippet}
</DocumentContent>
