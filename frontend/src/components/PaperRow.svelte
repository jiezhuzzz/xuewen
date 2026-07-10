<script lang="ts">
  import type { PaperSummary } from '../lib/types';
  import { openTab, searchMeta, viewer } from '../lib/state.svelte';
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
  {#if searchMeta.byId[paper.id]}
    {@const m = searchMeta.byId[paper.id]}
    <div class="mt-1 text-xs text-slate-600 dark:text-slate-300">
      <span class="mr-1 rounded bg-slate-100 px-1 py-px text-[10px] uppercase tracking-wide text-slate-500 dark:bg-slate-800 dark:text-slate-400">
        {m.field}{#if m.page != null}&nbsp;p.{m.page}{/if}
      </span>
      <!-- Server contract: snippet text is HTML-escaped; only <mark> tags. -->
      <span class="[&_mark]:rounded [&_mark]:bg-amber-200 [&_mark]:px-0.5 dark:[&_mark]:bg-amber-500/40">
        {@html m.snippet}
      </span>
    </div>
  {/if}
  <div class="mt-1.5 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
    {#if paper.year}<span>{paper.year}</span>{/if}
    {#if paper.venue}<span class="truncate">{#if paper.year}· {/if}{paper.venue}</span>{/if}
    <StatusPill status={paper.status} />
  </div>
</button>
