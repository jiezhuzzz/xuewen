<script lang="ts">
  import { ThumbImg, ThumbnailsPane, useThumbnailCapability } from '@embedpdf/plugin-thumbnail/svelte';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import { LayoutGrid, List } from 'lucide-svelte';
  import { reader, setPanelView } from '../lib/readerState.svelte';
  import PdfOutline from './PdfOutline.svelte';

  let { documentId }: { documentId: string } = $props();
  const scroll = useScroll(() => documentId);
  const thumbs = useThumbnailCapability();
  const tab = $derived(reader.panel[documentId] ?? null);

  function jump(pageIndex: number): void {
    scroll.provides?.scrollToPage({ pageNumber: pageIndex + 1 });
  }

  const seg = (active: boolean) =>
    `flex flex-1 items-center justify-center rounded-md px-2 py-1 ${
      active
        ? 'bg-parchment text-ink dark:bg-stone-800 dark:text-stone-100'
        : 'text-stone-500 hover:text-ink dark:text-stone-400 dark:hover:text-stone-100'
    }`;

  // Position the pane at the current page ONCE per thumbs-view activation
  // (continuous auto-follow is off — see pdfEngine.ts). scrollToThumb
  // silently no-ops until the plugin's thumb metadata and the pane's window
  // exist, and the pane's own scroll subscription attaches a beat after
  // mount — so repeat the (idempotent, instant) call across a few frames,
  // then stop. If the metadata never appears (broken doc), the pane simply
  // stays at the top.
  $effect(() => {
    if (tab !== 'thumbs') return;
    const scope = thumbs.provides?.forDocument(documentId);
    if (!scope) return;
    let tries = 0;
    let raf = 0;
    const attempt = () => {
      scope.scrollToThumb(scroll.state.currentPage - 1);
      if (++tries < 30) raf = requestAnimationFrame(attempt);
    };
    attempt();
    return () => cancelAnimationFrame(raf);
  });
</script>

<div class="flex w-44 shrink-0 flex-col border-r border-stone-200 bg-paper dark:border-stone-800 dark:bg-night">
  <div class="border-b border-stone-200 p-1.5 dark:border-stone-800">
    <div class="flex gap-0.5 rounded-lg bg-stone-100 p-0.5 dark:bg-stone-900">
      <button
        type="button"
        aria-label="Thumbnails"
        aria-pressed={tab === 'thumbs'}
        class={seg(tab === 'thumbs')}
        onclick={() => setPanelView(documentId, 'thumbs')}
      >
        <LayoutGrid size={14} />
      </button>
      <button
        type="button"
        aria-label="Outline"
        aria-pressed={tab === 'outline'}
        class={seg(tab === 'outline')}
        onclick={() => setPanelView(documentId, 'outline')}
      >
        <List size={14} />
      </button>
    </div>
  </div>
  {#if tab === 'thumbs'}
    <ThumbnailsPane {documentId} class="min-h-0 flex-1">
      {#snippet children(m)}
        <button
          type="button"
          aria-label={`Page ${m.pageIndex + 1}`}
          onclick={() => jump(m.pageIndex)}
          style:position="absolute"
          style:top="{m.top}px"
          style:left="50%"
          style:transform="translateX(-50%)"
          style:width="{m.width}px"
          style:height="{m.wrapperHeight}px"
        >
          <ThumbImg
            {documentId}
            meta={m}
            class={`rounded border ${
              scroll.state.currentPage === m.pageIndex + 1
                ? 'border-amber-700 ring-2 ring-amber-700/40 dark:border-amber-500 dark:ring-amber-500/40'
                : 'border-stone-200 dark:border-stone-700'
            }`}
          />
          <span
            class="block pt-0.5 text-center text-[10px] text-stone-500 dark:text-stone-400"
            style:height="{m.labelHeight}px"
          >
            {m.pageIndex + 1}
          </span>
        </button>
      {/snippet}
    </ThumbnailsPane>
  {:else if tab === 'outline'}
    <PdfOutline {documentId} />
  {/if}
</div>
