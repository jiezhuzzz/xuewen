<script lang="ts">
  import { untrack } from 'svelte';
  import { EmbedPDF } from '@embedpdf/core/svelte';
  import { usePdfiumEngine } from '@embedpdf/engines/svelte';
  import { pdfUrl } from '../lib/api';
  import { ENGINE_OPTIONS, viewerPlugins } from '../lib/pdfEngine';
  import PdfPages from './PdfPages.svelte';
  import CitationPopover from './CitationPopover.svelte';

  let { id, preference }: { id: string; preference: 'light' | 'dark'; active?: boolean } = $props();

  const engine = usePdfiumEngine(ENGINE_OPTIONS);
  // Plugins (and the document URL they point at) are built once for this
  // instance: id is fixed for the component's lifetime (PdfTab keys its
  // `{#each}` on tab.id, so a changed id remounts rather than updates).
  const plugins = untrack(() => viewerPlugins(pdfUrl(id)));
  // documentId is assigned by the document-manager plugin; we read the active
  // id off the EmbedPDF context and hand it down to PdfPages.
</script>

<div
  class="relative h-full w-full bg-stone-100 dark:bg-stone-950"
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
          <!-- Hoisted into its own component (not inlined here) so the
               citation-loading `$effect` + `useRegistry()` call can live in a
               component script — Svelte doesn't allow `$state`/`$effect`/hook
               calls directly inside a `{#snippet}` body. -->
          <PdfPages documentId={activeDocumentId} />
        {/if}
      {/snippet}
    </EmbedPDF>
  {/if}
  <CitationPopover />
</div>
