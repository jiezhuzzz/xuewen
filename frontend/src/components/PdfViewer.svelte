<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import { PDFViewer } from '@embedpdf/svelte-pdf-viewer';
  import { pdfUrl } from '../lib/api';
  import { theme, viewer } from '../lib/state.svelte';
  import { pdfViewerConfig, themePreference } from '../lib/pdfViewer';
  import PdfFallback from './PdfFallback.svelte';

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

  // `config.theme` only sets the viewer's INITIAL mode; it doesn't re-read the
  // prop when the app theme toggles. Capture the container on init and drive
  // `setTheme` so the viewer follows the app's light/dark live.
  type Container = Parameters<NonNullable<ComponentProps<typeof PDFViewer>['oninit']>>[0];
  let container = $state<Container | null>(null);
  $effect(() => {
    container?.setTheme(preference);
  });

  // `@embedpdf/svelte-pdf-viewer` exposes no load-error callback (only
  // `oninit`/`onready`), so failure is detected out-of-band: a HEAD check of
  // the PDF URL whenever the active paper changes. Reset the fallback first
  // so switching to a *working* paper doesn't keep showing the old failure.
  let failedId = $state<string | null>(null);
  $effect(() => {
    const id = viewer.activeId;
    failedId = null;
    container = null; // the current instance unmounts on activeId change; oninit re-sets it
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
          config={pdfViewerConfig(viewer.activeId, preference)}
          style="width:100%;height:100%"
          oninit={(c) => (container = c)}
        />
      {/key}
    {/if}
  {/if}
</div>
