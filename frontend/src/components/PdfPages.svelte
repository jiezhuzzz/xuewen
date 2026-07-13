<script lang="ts">
  import { useRegistry } from '@embedpdf/core/svelte';
  import { Viewport } from '@embedpdf/plugin-viewport/svelte';
  import { Scroller, type PageLayout } from '@embedpdf/plugin-scroll/svelte';
  import { DocumentContent } from '@embedpdf/plugin-document-manager/svelte';
  import { RenderLayer } from '@embedpdf/plugin-render/svelte';
  import { SelectionLayer } from '@embedpdf/plugin-selection/svelte';
  import { PagePointerProvider } from '@embedpdf/plugin-interaction-manager/svelte';
  import PdfControls from './PdfControls.svelte';
  import CitationLayer from './CitationLayer.svelte';
  import { loadCitations, type EngineLike } from '../lib/loadCitations';
  import { matchReferences } from '../lib/citationMatch';
  import { listPapers } from '../lib/api';
  import type { CitationData } from '../lib/citations';
  import type { PaperSummary } from '../lib/types';

  // Rendered as a child inside <EmbedPDF>'s `children` snippet (see
  // PdfDocViewer.svelte) so that `useRegistry()` — a context hook — resolves
  // against the plugin registry EmbedPDF sets up. Svelte doesn't allow
  // `$state`/`$effect`/hook calls directly inside a `{#snippet}` body (that's
  // template syntax, not a component script), so citation loading + the page
  // layout are hoisted into this dedicated component instead.
  let { documentId }: { documentId: string } = $props();

  const ctx = useRegistry();

  let citations = $state<CitationData>({ references: [], markers: [] });
  let matches = $state<Map<number, PaperSummary>>(new Map());
  let pageSizes = $state<{ width: number; height: number }[]>([]);

  // Extract citation markers + match them against the library once the
  // document is loaded. Extraction failures (e.g. an odd PDF structure) are
  // caught and logged so the reader still works without citation hovers.
  $effect(() => {
    const registry = ctx.registry;
    const docState = ctx.activeDocument;
    if (!registry || !docState?.document) return;
    const engine = registry.getEngine();
    const doc = docState.document;
    pageSizes = doc.pages.map((p) => ({ width: p.size.width, height: p.size.height }));
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
      <RenderLayer {documentId} pageIndex={page.pageIndex} />
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
      <p class="p-4 text-sm text-red-600">Failed to load document.</p>
    {:else}
      <p class="p-4 text-sm text-stone-500">Loading document…</p>
    {/if}
  {/snippet}
</DocumentContent>
