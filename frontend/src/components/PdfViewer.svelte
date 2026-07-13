<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import { PDFViewer } from '@embedpdf/svelte-pdf-viewer';
  import { pdfUrl } from '../lib/api';
  import { theme, viewer } from '../lib/state.svelte';
  import { pdfViewerConfig, themePreference } from '../lib/pdfViewer';
  import PdfFallback from './PdfFallback.svelte';

  type ViewerConfig = ComponentProps<typeof PDFViewer>['config'];

  // Effective dark for `system` mode. Reactive to explicit theme changes; the
  // OS-follow case updates on the media-query event below.
  let systemDark = $state(
    typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches,
  );
  $effect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  });

  const preference = $derived(themePreference(theme.mode, systemDark));

  // `@embedpdf/svelte-pdf-viewer` exposes no load-error callback (only
  // `oninit`/`onready`), so failure is detected out-of-band: a HEAD check of
  // the PDF URL whenever the active paper changes. Reset the fallback first
  // so switching to a *working* paper doesn't keep showing the old failure.
  let failedId = $state<string | null>(null);
  $effect(() => {
    const id = viewer.activeId;
    failedId = null;
    if (!id) return;
    const controller = new AbortController();
    fetch(pdfUrl(id), { method: 'HEAD', signal: controller.signal })
      .then((res) => {
        if (!res.ok) failedId = id;
      })
      .catch((err) => {
        if (err instanceof DOMException && err.name === 'AbortError') return;
        failedId = id;
      });
    return () => controller.abort();
  });
</script>

<div class="relative min-h-0 flex-1 bg-stone-100 dark:bg-stone-950">
  {#if viewer.activeId}
    {#if failedId === viewer.activeId}
      <PdfFallback id={viewer.activeId} />
    {:else}
      {#key viewer.activeId}
        <PDFViewer
          config={pdfViewerConfig(viewer.activeId, preference) as unknown as ViewerConfig}
          style="width:100%;height:100%"
        />
      {/key}
    {/if}
  {/if}
</div>
