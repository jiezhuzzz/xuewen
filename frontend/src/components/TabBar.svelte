<script lang="ts">
  import { Info, X } from 'lucide-svelte';
  import { closeTab, viewer } from '../lib/state.svelte';
</script>

<div class="flex h-11 shrink-0 items-center border-b border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
  <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
    {#each viewer.tabs as tab (tab.id)}
      <div
        class={`group flex h-11 max-w-52 shrink-0 items-center gap-2 border-r border-slate-200 px-3 dark:border-slate-800 ${
          viewer.activeId === tab.id
            ? 'bg-slate-50 dark:bg-slate-800/60'
            : 'hover:bg-slate-50 dark:hover:bg-slate-800/30'
        }`}
      >
        <button
          type="button"
          onclick={() => (viewer.activeId = tab.id)}
          class="min-w-0 truncate text-sm text-slate-700 dark:text-slate-200"
        >
          {tab.title}
        </button>
        <button
          type="button"
          aria-label="Close tab"
          onclick={() => closeTab(tab.id)}
          class="rounded p-0.5 text-slate-500 opacity-0 hover:bg-slate-200 group-hover:opacity-100 dark:text-slate-400 dark:hover:bg-slate-700"
        >
          <X size={14} />
        </button>
      </div>
    {/each}
  </div>
  <button
    type="button"
    aria-label="Toggle info"
    onclick={() => (viewer.infoOpen = !viewer.infoOpen)}
    class={`mr-2 shrink-0 rounded-lg p-2 ${
      viewer.infoOpen
        ? 'bg-indigo-50 text-indigo-600 dark:bg-indigo-500/15 dark:text-indigo-400'
        : 'text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800'
    }`}
  >
    <Info size={18} />
  </button>
</div>
