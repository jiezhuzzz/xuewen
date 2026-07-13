<script lang="ts">
  import { EmbedPDF } from '@embedpdf/core/svelte';
  import { usePdfiumEngine } from '@embedpdf/engines/svelte';
  import { ENGINE_OPTIONS, viewerPlugins } from '../lib/pdfEngine';
  import PdfDeck from './PdfDeck.svelte';

  // ONE app-level engine + <EmbedPDF>. @embedpdf/core's Svelte bindings use a
  // module-level singleton context, so only a single <EmbedPDF> can exist per
  // page; every open paper is a document within this one registry (see PdfDeck).
  const engine = usePdfiumEngine(ENGINE_OPTIONS);
  const plugins = viewerPlugins();
</script>

<div class="relative min-h-0 flex-1 bg-stone-100 dark:bg-stone-950">
  {#if engine.isLoading}
    <p class="p-4 text-sm text-stone-500 dark:text-stone-400">Loading PDF engine…</p>
  {:else if engine.error}
    <p class="p-4 text-sm text-red-600 dark:text-red-400">Engine failed: {engine.error.message}</p>
  {:else if engine.engine}
    <EmbedPDF engine={engine.engine} {plugins}>
      {#snippet children()}
        <PdfDeck />
      {/snippet}
    </EmbedPDF>
  {/if}
</div>
