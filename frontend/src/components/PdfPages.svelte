<script lang="ts">
  import { useRegistry, useDocumentState } from '@embedpdf/core/svelte';
  import { Viewport } from '@embedpdf/plugin-viewport/svelte';
  import { Scroller, type PageLayout } from '@embedpdf/plugin-scroll/svelte';
  import { DocumentContent } from '@embedpdf/plugin-document-manager/svelte';
  import { RenderLayer } from '@embedpdf/plugin-render/svelte';
  import { SelectionLayer } from '@embedpdf/plugin-selection/svelte';
  import { PagePointerProvider } from '@embedpdf/plugin-interaction-manager/svelte';
  import { TilingLayer } from '@embedpdf/plugin-tiling/svelte';
  import PdfControls from './PdfControls.svelte';
  import CitationLayer from './CitationLayer.svelte';
  import { loadCitations, type EngineLike } from '../lib/loadCitations';
  import { matchReferences } from '../lib/citationMatch';
  import { listPapers } from '../lib/api';
  import type { CitationData } from '../lib/citations';
  import type { PaperSummary } from '../lib/types';

  // Renders one paper's pages inside the shared <EmbedPDF> (see PdfViewer/PdfDeck).
  // Bound to its own `documentId` — one PdfPages is mounted per open tab — so the
  // shared engine is fine while each tab reads/extracts its own document.
  let { documentId }: { documentId: string } = $props();

  const ctx = useRegistry();
  const docState = useDocumentState(() => documentId);

  let citations = $state<CitationData>({ references: [], markers: [] });
  let matches = $state<Map<number, PaperSummary>>(new Map());
  let pageSizes = $state<{ width: number; height: number }[]>([]);

  // Extract citation markers + match them against the library ONCE per document.
  // `docState.current` (and its `.document`) is reassigned on any core change —
  // incl. zoom scale and an initial load→reload — so guarding on the document
  // object's identity still re-ran extraction. Guard on the (fixed) documentId
  // instead: one PdfPages is mounted per tab, so extraction runs exactly once.
  // Failures are caught/logged so the reader still works without citation hovers.
  let extractedFor: string | null = null;
  $effect(() => {
    const registry = ctx.registry;
    const doc = docState.current?.document ?? null;
    if (!registry || !doc || extractedFor === documentId) return;
    extractedFor = documentId;
    pageSizes = doc.pages.map((p) => ({ width: p.size.width, height: p.size.height }));
    const engine = registry.getEngine();
    let cancelled = false;
    (async () => {
      try {
        const data = await loadCitations(engine as unknown as EngineLike, doc);
        if (cancelled) return;
        citations = data;
        // Whole library, independent of the current UI filter.
        const papers = await listPapers({ q: '', status: 'all', sort: 'year_desc', project: 'all' });
        if (cancelled) return;
        matches = matchReferences(data.references, papers);
      } catch (err) {
        console.warn('citation extraction failed', err); // reader still works
      }
    })();
    return () => {
      cancelled = true;
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
      <RenderLayer {documentId} pageIndex={page.pageIndex} scale={1} class="pointer-events-none" />
      <TilingLayer {documentId} pageIndex={page.pageIndex} class="pointer-events-none" />
      <SelectionLayer {documentId} pageIndex={page.pageIndex} />
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

<DocumentContent {documentId}>
  {#snippet children(doc)}
    {#if doc.isLoaded}
      <PdfControls {documentId} />
      <Viewport {documentId} class="h-full w-full">
        <Scroller {documentId} {renderPage} />
      </Viewport>
    {:else if doc.isError}
      <p class="p-4 text-sm text-red-600 dark:text-red-400">Failed to load document.</p>
    {:else}
      <p class="p-4 text-sm text-stone-500 dark:text-stone-400">Loading document…</p>
    {/if}
  {/snippet}
</DocumentContent>
