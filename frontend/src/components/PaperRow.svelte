<script lang="ts">
  import type { PaperSummary } from '../lib/types';
  import { openContextMenu } from '../lib/contextMenu.svelte';
  import { openTab, searchMeta, selectPaper, selection, toggleStar, viewer } from '../lib/state.svelte';
  import { abbreviateVenue } from '../lib/venue';
  import PaperRowTags from './PaperRowTags.svelte';
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

  // A single click (or Enter/Space — see the non-native button role below)
  // opens the paper's PDF (openTab also highlights the row).
  function open() {
    openTab(paper);
  }
  function onKeydown(e: KeyboardEvent) {
    // Only act on keys that landed on the row itself. keydown bubbles up from
    // the nested star / +N buttons; without this guard the row's preventDefault
    // would swallow the browser's synthetic click on those buttons, leaving
    // them keyboard-dead (Enter/Space would open the paper instead of firing
    // their own action).
    if (e.target !== e.currentTarget) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      open();
    }
  }
  // The star toggle must be a real nested <button> (for native focus/click
  // and aria-pressed semantics), so the row itself can't be a <button> too:
  // browsers pop an open <button> off the stack when a nested one starts
  // (an explicit HTML parser rule), which would silently hoist the star out
  // of the row instead of nesting it. A div w/ role="button" + tabindex +
  // keydown keeps the row keyboard-activatable without that trap.
  function onStarClick(e: MouseEvent) {
    e.stopPropagation();
    void toggleStar(paper.id);
  }
  // Right-click highlights the row (so it's clear which paper the menu targets)
  // and opens the shared context menu — without opening the PDF (left-click's job).
  function onContextMenu(e: MouseEvent) {
    selectPaper(paper.id);
    openContextMenu(e, paper);
  }
</script>

<div
  role="button"
  tabindex="0"
  onclick={open}
  oncontextmenu={onContextMenu}
  onkeydown={onKeydown}
  class={`group relative w-full cursor-pointer border-l-2 px-4 py-3 text-left transition-colors hover:bg-parchment dark:hover:bg-stone-800/50 ${
    selected ? 'border-amber-700 bg-parchment dark:border-amber-500 dark:bg-stone-800/50' : 'border-transparent'
  }`}
>
  <!-- Star pinned to the row's top-right corner so it never indents the title.
       Starred papers show it filled; unstarred rows keep it in the DOM (the
       star button is how you star from the list) but hidden until row-hover or
       keyboard focus — no resting empty star cluttering every row. -->
  <button
    type="button"
    aria-label={paper.starred ? 'Unstar paper' : 'Star paper'}
    aria-pressed={paper.starred}
    onclick={onStarClick}
    class={`absolute right-3 top-3 shrink-0 text-xs leading-none transition-opacity ${
      paper.starred
        ? 'text-orange-600 dark:text-orange-400'
        : 'text-stone-300 opacity-0 hover:text-stone-400 focus-visible:opacity-100 group-hover:opacity-100 dark:text-stone-600 dark:hover:text-stone-500'
    }`}
  >★</button>
  <div class="line-clamp-2 pr-5 font-serif text-sm font-medium text-ink dark:text-stone-100">
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
  <PaperRowTags {paper} />
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
</div>
