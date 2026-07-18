<script lang="ts">
  import { onDestroy } from 'svelte';
  import { useRegistry, useDocumentState } from '@embedpdf/core/svelte';
  import { Viewport } from '@embedpdf/plugin-viewport/svelte';
  import { Scroller, type PageLayout } from '@embedpdf/plugin-scroll/svelte';
  import { ZoomGestureWrapper } from '@embedpdf/plugin-zoom/svelte';
  import { DocumentContent } from '@embedpdf/plugin-document-manager/svelte';
  import { RenderLayer } from '@embedpdf/plugin-render/svelte';
  import { SelectionLayer, useSelectionCapability } from '@embedpdf/plugin-selection/svelte';
  import { GlobalPointerProvider, PagePointerProvider } from '@embedpdf/plugin-interaction-manager/svelte';
  import { TilingLayer } from '@embedpdf/plugin-tiling/svelte';
  import PdfToolbar from './PdfToolbar.svelte';
  import PdfQuickActions from './PdfQuickActions.svelte';
  import PdfFindBar from './PdfFindBar.svelte';
  import PdfSidePanel from './PdfSidePanel.svelte';
  import TranslateBubble from './TranslateBubble.svelte';
  import Spinner from './Spinner.svelte';
  import { SearchLayer } from '@embedpdf/plugin-search/svelte';
  import CitationLayer from './CitationLayer.svelte';
  import { loadCitations, type EngineLike } from '../lib/loadCitations';
  import { libraryTitleIndex, matchReferences } from '../lib/citationMatch';
  import { parseCitations } from '../lib/api';
  import { runWhenIdle } from '../lib/idle';
  import { mergeStructured } from '../lib/refMerge';
  import { resolveAuthorYearMarkers } from '../lib/textCitations';
  import { reader } from '../lib/readerState.svelte';
  import { pdfAppearance } from '../lib/state.svelte';
  import { createPillHide } from '../lib/pillHide.svelte';
  import { Spring } from 'svelte/motion';
  import { prefersReducedMotion, SPRINGS } from '../lib/motion';
  import { appSettings } from '../lib/state.svelte';
  import { requestTranslate, translateTrigger } from '../lib/translate.svelte';
  import type { CitationData } from '../lib/citations';
  import type { PaperSummary } from '../lib/types';

  // Renders one paper's pages inside the shared <EmbedPDF> (see PdfViewer/PdfDeck).
  // Bound to its own `documentId` — one PdfPages is mounted per open tab — so the
  // shared engine is fine while each tab reads/extracts its own document.
  let { documentId }: { documentId: string } = $props();

  const ctx = useRegistry();
  const docState = useDocumentState(() => documentId);

  // Selection → translate wiring (Task 7). `getSelectedText()` takes only an
  // optional documentId — no doc/page object crosses into the engine call —
  // so the $state.snapshot/DataCloneError gotcha above does not apply here.
  const selectionCap = useSelectionCapability();
  let lastPointer = $state<{ x: number; y: number } | null>(null);
  let bubble = $state<{ x: number; y: number; text: string } | null>(null);

  // Shared zen auto-hide for the floating pills (see lib/pillHide.svelte.ts).
  const pill = createPillHide(() => documentId);
  let pillHost = $state<HTMLDivElement | undefined>();
  $effect(() => {
    pill.setHost(pillHost ?? null);
  });

  // Animated panel push (the library-pane idiom — see App.svelte): the
  // wrapper's width springs 0↔PANEL_W so the PDF eases sideways instead of
  // jumping when the sidebar toggles.
  const PANEL_W = 176; // = w-44
  // svelte-ignore state_referenced_locally -- initial value only; the
  // $effect below drives every subsequent update via panelW.target.
  const panelW = new Spring(reader.panel ? PANEL_W : 0, SPRINGS.pane);
  $effect(() => {
    const target = reader.panel ? PANEL_W : 0;
    if (import.meta.env.MODE === 'test' || prefersReducedMotion()) {
      panelW.set(target, { instant: true });
    } else {
      panelW.target = target;
    }
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

  // Subscribe to selection-end while the translate feature is on. Auto mode
  // fires requestTranslate immediately; Manual mode shows the bubble instead,
  // which the user must click (see TranslateBubble.svelte). onEndSelection's
  // event carries no text — getSelectedText() is fetched separately per the
  // plugin's PdfTask API (same .toPromise() pattern as loadCitations.ts).
  // Anchored at the window-level capture-phase pointerup (svelte:window handler
  // above), not page-space math — the selection plugin can stop bubbling, so
  // a div-level handler would leave lastPointer stale. useSelectionCapability()
  // is a registry-wide SINGLETON, not per-document — every open tab keeps its
  // own (hidden) PdfPages mounted and running this effect, so every tab's
  // listener fires on ANY tab's selection. Guard on documentId (both the
  // event and the getSelectedText call) so only the tab that owns the
  // selection reacts. The anchor coords are captured synchronously, before
  // the await, so a pointer event during the await can't move them.
  $effect(() => {
    const cap = selectionCap.provides;
    if (!cap || !appSettings.translate.enabled) return;
    const unsub = cap.onEndSelection((ev) => {
      if (ev.documentId !== documentId) return;
      const at = lastPointer ?? { x: window.innerWidth / 2, y: 200 };
      void (async () => {
        const parts = await cap.getSelectedText(documentId).toPromise();
        const text = (parts ?? []).join(' ').trim();
        if (!text) {
          bubble = null;
          return;
        }
        if (translateTrigger() === 'auto') {
          bubble = null;
          void requestTranslate(text, at);
        } else {
          bubble = { x: at.x, y: at.y, text };
        }
      })();
    });
    return () => {
      unsub?.();
      bubble = null;
    };
  });
</script>

{#snippet renderPage(page: PageLayout)}
  <div style:width="{page.width}px" style:height="{page.height}px" style:position="relative">
    <PagePointerProvider {documentId} pageIndex={page.pageIndex}>
      <!-- Low-res base rendered once (scale locked at 1, CSS-scaled by the
           framework); TilingLayer draws crisp visible tiles at the real zoom.
           This mirrors the ready-made viewer and is the perf fix — do NOT
           remove scale={1} or pages re-render fully on every zoom. -->
      <!-- Dark-mode dim/invert (app.css, .dark-scoped) wraps ONLY the raster
           layers: selection/search highlights and citation overlays keep
           their true colors, and nothing position:fixed lives under the
           filter (a filter creates a new containing block). -->
      <div class="pointer-events-none absolute inset-0" data-pdf-appearance={pdfAppearance.mode}>
        <RenderLayer {documentId} pageIndex={page.pageIndex} scale={1} class="pointer-events-none" />
        <TilingLayer {documentId} pageIndex={page.pageIndex} class="pointer-events-none" />
      </div>
      <SelectionLayer {documentId} pageIndex={page.pageIndex} />
      <SearchLayer
        {documentId}
        pageIndex={page.pageIndex}
        class="pointer-events-none"
        highlightColor="rgba(180, 83, 9, 0.28)"
        activeHighlightColor="rgba(180, 83, 9, 0.55)"
      />
      <CitationLayer
        {documentId}
        pageIndex={page.pageIndex}
        pageWidthPt={pageSizes[page.pageIndex]?.width ?? page.width}
        pageHeightPt={pageSizes[page.pageIndex]?.height ?? page.height}
        data={citations}
        {matches}
      />
    </PagePointerProvider>
  </div>
{/snippet}

<svelte:window
  onpointermove={(e) => pill.onWindowMove(e)}
  onpointerupcapture={(e) => (lastPointer = { x: e.clientX, y: e.clientY })}
/>

<DocumentContent {documentId}>
  {#snippet children(doc)}
    {#if doc.isLoaded}
      <div class="flex h-full">
        {#if reader.panel || panelW.current > 1}
          <!-- Kept mounted while the spring settles so closing slides the
               panel away instead of blanking it; inert once logically closed.
               A rapid close→reopen within the settle window intentionally
               skips re-positioning/reveal: the panel never unmounts and `tab`
               never changes, so the user keeps their browse position — this
               is deliberate, not a missed one-shot. -->
          <div
            class="relative min-h-0 shrink-0 overflow-hidden"
            style={`width:${panelW.current}px`}
            inert={!reader.panel}
          >
            <div class="absolute inset-y-0 left-0 flex w-44">
              <PdfSidePanel {documentId} />
            </div>
          </div>
        {/if}
        <div class="relative min-w-0 flex-1" bind:this={pillHost}>
          <PdfToolbar {documentId} {pill} />
          <PdfQuickActions {pill} />
          {#if reader.find[documentId]}
            <PdfFindBar {documentId} />
          {/if}
          <!-- Zoom/scroll/pinch wiring mirrors EmbedPDF's own ready-made viewer
               (viewers/snippet app.tsx): GlobalPointerProvider > Viewport >
               ZoomGestureWrapper > Scroller, all stock. -->
          <GlobalPointerProvider {documentId}>
            <Viewport {documentId} class="h-full w-full">
              <!-- No class on ZoomGestureWrapper: it must size to its content,
                   not the viewport. Its pinch-anchor math reads the wrapped
                   element's own width/height, so forcing h-full/w-full (element
                   = viewport size, while the content is many pages tall) breaks
                   the anchor — pinching a corner scaled toward the opposite one.
                   EmbedPDF's own viewer passes no class here. -->
              <ZoomGestureWrapper {documentId}>
                <Scroller {documentId} {renderPage} />
              </ZoomGestureWrapper>
            </Viewport>
          </GlobalPointerProvider>
        </div>
      </div>
      {#if bubble}
        <TranslateBubble
          x={bubble.x}
          y={bubble.y}
          onTranslate={() => {
            const b = bubble;
            bubble = null;
            if (b) void requestTranslate(b.text, { x: b.x, y: b.y });
          }}
        />
      {/if}
    {:else if doc.isError}
      <p class="p-4 text-sm text-red-600 dark:text-red-400">Failed to load document.</p>
    {:else}
      <!-- Centered, not a corner note: the blank page area otherwise reads
           as broken during the multi-second worker boot + first parse. -->
      <div class="flex h-full items-center justify-center">
        <Spinner label="Loading document…" />
      </div>
    {/if}
  {/snippet}
</DocumentContent>
