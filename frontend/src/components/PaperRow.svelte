<script lang="ts">
  import type { PaperSummary } from '../lib/types';
  import { openTab, viewer } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';

  let { paper }: { paper: PaperSummary } = $props();
  const active = $derived(viewer.activeId === paper.id);
  const authors = $derived(
    paper.authors.length > 3
      ? `${paper.authors.slice(0, 3).join(', ')} et al.`
      : paper.authors.join(', '),
  );
</script>

<button
  type="button"
  onclick={() => openTab(paper)}
  class={`w-full border-l-2 px-4 py-3 text-left transition hover:bg-slate-50 dark:hover:bg-slate-800/50 ${
    active ? 'border-indigo-500 bg-slate-50 dark:bg-slate-800/50' : 'border-transparent'
  }`}
>
  <div class="line-clamp-2 text-sm font-medium text-slate-900 dark:text-slate-100">
    {paper.title ?? '(untitled)'}
  </div>
  {#if authors}
    <div class="mt-0.5 line-clamp-1 text-xs text-slate-500 dark:text-slate-400">{authors}</div>
  {/if}
  <div class="mt-1.5 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
    {#if paper.year}<span>{paper.year}</span>{/if}
    {#if paper.venue}<span class="truncate">{#if paper.year}· {/if}{paper.venue}</span>{/if}
    <StatusPill status={paper.status} />
  </div>
</button>
