<script lang="ts">
  import { untrack } from 'svelte';
  import { EmbedPDF } from '@embedpdf/core/svelte';
  import { usePdfiumEngine } from '@embedpdf/engines/svelte';
  import { Viewport } from '@embedpdf/plugin-viewport/svelte';
  import { Scroller, type PageLayout } from '@embedpdf/plugin-scroll/svelte';
  import { DocumentContent } from '@embedpdf/plugin-document-manager/svelte';
  import { RenderLayer } from '@embedpdf/plugin-render/svelte';
  import { SelectionLayer } from '@embedpdf/plugin-selection/svelte';
  import { PagePointerProvider } from '@embedpdf/plugin-interaction-manager/svelte';
  import { pdfUrl } from '../lib/api';
  import { ENGINE_OPTIONS, viewerPlugins } from '../lib/pdfEngine';

  let { id, preference }: { id: string; preference: 'light' | 'dark'; active?: boolean } = $props();

  const engine = usePdfiumEngine(ENGINE_OPTIONS);
  // Plugins (and the document URL they point at) are built once for this
  // instance: id is fixed for the component's lifetime (PdfTab keys its
  // `{#each}` on tab.id, so a changed id remounts rather than updates).
  const plugins = untrack(() => viewerPlugins(pdfUrl(id)));
  // documentId defaults to the loaded doc; DocumentContent/Viewport need it. The
  // document-manager assigns one; we read the active id from the EmbedPDF context.
</script>

<div
  class="h-full w-full bg-stone-100 dark:bg-stone-950"
  data-theme={preference}
>
  {#if engine.isLoading}
    <p class="p-4 text-sm text-stone-500">Loading PDF engine…</p>
  {:else if engine.error}
    <p class="p-4 text-sm text-red-600">Engine failed: {engine.error.message}</p>
  {:else if engine.engine}
    <EmbedPDF engine={engine.engine} {plugins}>
      {#snippet children({ activeDocumentId })}
        {#if activeDocumentId}
          {@const documentId = activeDocumentId}
          {#snippet renderPage(page: PageLayout)}
            <div style:width="{page.width}px" style:height="{page.height}px" style:position="relative">
              <PagePointerProvider {documentId} pageIndex={page.pageIndex}>
                <RenderLayer {documentId} pageIndex={page.pageIndex} />
                <SelectionLayer {documentId} pageIndex={page.pageIndex} />
              </PagePointerProvider>
            </div>
          {/snippet}
          <DocumentContent {documentId}>
            {#snippet children(doc)}
              {#if doc.isLoaded}
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
        {/if}
      {/snippet}
    </EmbedPDF>
  {/if}
</div>
