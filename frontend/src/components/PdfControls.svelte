<script lang="ts">
  import { ZoomIn, ZoomOut, Maximize } from 'lucide-svelte';
  import { useZoomCapability } from '@embedpdf/plugin-zoom/svelte';
  import { ZoomMode } from '@embedpdf/plugin-zoom';

  let { documentId }: { documentId: string } = $props();
  const zoom = useZoomCapability();

  function scope() {
    return zoom.provides?.forDocument(documentId);
  }
  const btn =
    'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<div class="absolute right-3 top-3 z-20 flex items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow dark:border-stone-800 dark:bg-soot/90">
  <button class={btn} aria-label="Zoom out" onclick={() => scope()?.zoomOut()}><ZoomOut size={16} /></button>
  <button class={btn} aria-label="Fit width" onclick={() => scope()?.requestZoom(ZoomMode.FitWidth)}><Maximize size={16} /></button>
  <button class={btn} aria-label="Zoom in" onclick={() => scope()?.zoomIn()}><ZoomIn size={16} /></button>
</div>
