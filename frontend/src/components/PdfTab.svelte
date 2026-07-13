<script lang="ts">
  import { pdfUrl } from '../lib/api';
  import PdfDocViewer from './PdfDocViewer.svelte';
  import PdfFallback from './PdfFallback.svelte';

  let { id, preference, active }: { id: string; preference: 'light' | 'dark'; active: boolean } =
    $props();

  // One-time HEAD check for a missing/broken PDF (id is fixed for this instance).
  let failed = $state(false);
  $effect(() => {
    const controller = new AbortController();
    fetch(pdfUrl(id), { method: 'HEAD', signal: controller.signal })
      .then((res) => {
        if (!res.ok) failed = true;
      })
      .catch((err) => {
        if (err instanceof DOMException && err.name === 'AbortError') return;
        failed = true;
      });
    return () => controller.abort();
  });
</script>

<!-- Hide inactive tabs with visibility (not display:none): display:none
     collapses the tab to 0×0, which resets EmbedPDF's virtualized thumbnail
     list to the top and makes it re-scroll (animate) to the current page on
     every switch. `invisible` keeps the layout, so state is fully preserved. -->
<div class={`absolute inset-0 ${active ? 'z-10' : 'invisible'}`}>
  {#if failed}
    <PdfFallback {id} />
  {:else}
    <PdfDocViewer {id} {preference} {active} />
  {/if}
</div>
