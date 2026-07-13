<script lang="ts">
  import type { PaperSummary } from '../lib/types';
  import { openTab, searchMeta, selection, viewer } from '../lib/state.svelte';
  import { abbreviateVenue } from '../lib/venue';
  import StatusPill from './StatusPill.svelte';

  let { paper }: { paper: PaperSummary } = $props();
  const venueLabel = $derived(abbreviateVenue(paper.venue));
  const selected = $derived(selection.id === paper.id);
  const isOpen = $derived(viewer.tabs.some((t) => t.id === paper.id));
  // With 3+ authors, show just the first and last (middle authors elided).
  const authors = $derived(
    paper.authors.length > 2
      ? `${paper.authors[0]} … ${paper.authors[paper.authors.length - 1]}`
      : paper.authors.join(', '),
  );

  // A single click opens the paper's PDF (openTab also highlights the row).
  function open() {
    openTab(paper);
  }
</script>

<button
  type="button"
  onclick={open}
  class={`w-full border-l-2 px-4 py-3 text-left transition-colors hover:bg-parchment dark:hover:bg-stone-800/50 ${
    selected ? 'border-amber-700 bg-parchment dark:border-amber-500 dark:bg-stone-800/50' : 'border-transparent'
  }`}
>
  <div class="line-clamp-2 font-serif text-sm font-medium text-ink dark:text-stone-100">
    {paper.title ?? '(untitled)'}
    {#if isOpen}
      <span
        title="Open in a tab"
        class="ml-1 inline-block h-1.5 w-1.5 rounded-full bg-amber-700 align-middle dark:bg-amber-500"
      ></span>
    {/if}
  </div>
  {#if authors}
    <div class="mt-0.5 line-clamp-1 text-xs text-stone-500 dark:text-stone-400">{authors}</div>
  {/if}
  {#if searchMeta.byId[paper.id]}
    {@const m = searchMeta.byId[paper.id]}
    <div class="mt-1 text-xs text-stone-600 dark:text-stone-300">
      <span class="mr-1 rounded bg-parchment px-1 py-px font-mono text-[10px] uppercase tracking-wide text-stone-500 dark:bg-stone-800 dark:text-stone-400">
        {m.field}{#if m.page != null}&nbsp;p.{m.page}{/if}
      </span>
      <!-- Server contract: snippet text is HTML-escaped; only <mark> tags. -->
      <span class="[&_mark]:rounded [&_mark]:bg-yellow-200 [&_mark]:px-0.5 dark:[&_mark]:bg-yellow-500/40">
        {@html m.snippet}
      </span>
    </div>
  {/if}
  <div class="mt-1.5 flex items-center gap-2 text-xs text-stone-500 dark:text-stone-400">
    {#if paper.year}<span>{paper.year}</span>{/if}
    {#if paper.year && paper.venue}<span aria-hidden="true" class="-mx-1">•</span>{/if}
    {#if paper.venue}<span class="truncate" title={paper.venue}>{venueLabel}</span>{/if}
    <StatusPill status={paper.status} />
  </div>
</button>
