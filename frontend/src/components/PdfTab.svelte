<script lang="ts">
  import { untrack, type ComponentProps } from 'svelte';
  import { PDFViewer } from '@embedpdf/svelte-pdf-viewer';
  import { pdfUrl } from '../lib/api';
  import { pdfViewerConfig } from '../lib/pdfViewer';
  import PdfFallback from './PdfFallback.svelte';

  let { id, preference, active }: { id: string; preference: 'light' | 'dark'; active: boolean } =
    $props();

  // One persistent viewer instance per open tab (kept mounted, just hidden when
  // inactive) so switching tabs never remounts/reloads the PDF or resets the
  // page. The config is built once with the initial theme; live theme changes
  // go through setTheme below (rebuilding config would reload the document).
  const config = untrack(() => pdfViewerConfig(id, preference));

  type Container = Parameters<NonNullable<ComponentProps<typeof PDFViewer>['oninit']>>[0];
  let container = $state<Container | null>(null);
  $effect(() => {
    container?.setTheme(preference);
  });

  // The viewer has no load-error callback, so detect a missing/broken PDF with
  // a one-time HEAD check (id is fixed for this instance).
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
    <PDFViewer {config} style="width:100%;height:100%" oninit={(c) => (container = c)} />
  {/if}
</div>
