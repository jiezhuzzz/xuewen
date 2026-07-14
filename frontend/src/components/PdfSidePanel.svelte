<script lang="ts">
  import { ThumbImg, ThumbnailsPane } from '@embedpdf/plugin-thumbnail/svelte';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import { reader } from '../lib/readerState.svelte';
  import PdfOutline from './PdfOutline.svelte';

  let { documentId }: { documentId: string } = $props();
  const scroll = useScroll(() => documentId);
  const tab = $derived(reader.panel[documentId] ?? null);

  function jump(pageIndex: number): void {
    scroll.provides?.scrollToPage({ pageNumber: pageIndex + 1 });
  }
</script>

<div class="flex w-44 shrink-0 flex-col border-r border-stone-200 bg-paper dark:border-stone-800 dark:bg-night">
  <div class="flex gap-1 border-b border-stone-200 p-1.5 dark:border-stone-800">
    <button
      type="button"
      aria-pressed={tab === 'thumbs'}
      onclick={() => (reader.panel[documentId] = 'thumbs')}
      class={`flex-1 rounded-md px-2 py-1 text-xs ${
        tab === 'thumbs'
          ? 'bg-parchment text-ink dark:bg-stone-800 dark:text-stone-100'
          : 'text-stone-500 hover:text-ink dark:text-stone-400 dark:hover:text-stone-100'
      }`}
    >
      Pages
    </button>
    <button
      type="button"
      aria-pressed={tab === 'outline'}
      onclick={() => (reader.panel[documentId] = 'outline')}
      class={`flex-1 rounded-md px-2 py-1 text-xs ${
        tab === 'outline'
          ? 'bg-parchment text-ink dark:bg-stone-800 dark:text-stone-100'
          : 'text-stone-500 hover:text-ink dark:text-stone-400 dark:hover:text-stone-100'
      }`}
    >
      Outline
    </button>
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
