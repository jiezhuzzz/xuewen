<script lang="ts">
  import { FileText } from 'lucide-svelte';
  import { library, openTab } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';
</script>

<div class="h-full overflow-y-auto p-8">
  <div class="mx-auto max-w-5xl">
    <div class="mb-6 flex items-center gap-2 text-slate-500 dark:text-slate-400">
      <FileText size={18} />
      <p class="text-sm">Open a paper to read it inline. You can keep several open as tabs.</p>
    </div>
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
      {#each library.papers as paper (paper.id)}
        <button
          type="button"
          onclick={() => openTab(paper)}
          class="rounded-xl border border-slate-200 bg-white p-4 text-left shadow-sm transition hover:-translate-y-0.5 hover:shadow-md dark:border-slate-800 dark:bg-slate-900"
        >
          <div class="line-clamp-2 font-medium">{paper.title ?? '(untitled)'}</div>
          <div class="mt-1 line-clamp-1 text-xs text-slate-500 dark:text-slate-400">
            {paper.authors.slice(0, 3).join(', ')}{paper.authors.length > 3 ? ' et al.' : ''}
          </div>
          <div class="mt-2 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
            {#if paper.year}<span>{paper.year}</span>{/if}
            <StatusPill status={paper.status} />
          </div>
        </button>
      {/each}
    </div>
  </div>
</div>
