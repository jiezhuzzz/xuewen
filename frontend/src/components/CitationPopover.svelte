<script lang="ts">
  import { ExternalLink, BookOpen } from 'lucide-svelte';
  import { citationHover, cancelHideCitation, hideCitationSoon } from '../lib/citationState.svelte';
  import { openTab } from '../lib/state.svelte';

  const c = $derived(citationHover.current);

  function open() {
    if (c?.matchedPaper) openTab(c.matchedPaper);
    citationHover.current = null;
  }
</script>

{#if c}
  <div
    role="tooltip"
    onpointerenter={cancelHideCitation}
    onpointerleave={hideCitationSoon}
    style:left="{c.screenX}px"
    style:top="{c.screenY}px"
    class="pointer-events-auto fixed z-50 max-w-sm -translate-y-full rounded-xl border border-stone-200 bg-paper p-3 text-[12.5px] shadow-2xl dark:border-stone-800 dark:bg-soot"
  >
    <p class="font-serif leading-relaxed text-stone-700 dark:text-stone-300">{c.reference.rawText}</p>
    <div class="mt-2 flex items-center gap-3">
      {#if c.matchedPaper}
        <button
          type="button"
          onclick={open}
          class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
        >
          <BookOpen size={13} /> Open in library
        </button>
      {/if}
      {#if c.reference.externalUrl}
        <a
          href={c.reference.externalUrl}
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center gap-1 text-xs text-stone-500 hover:text-ink dark:text-stone-400"
        >
          <ExternalLink size={12} /> {new URL(c.reference.externalUrl).host}
        </a>
      {/if}
    </div>
  </div>
{/if}
