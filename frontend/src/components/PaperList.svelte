<script lang="ts">
  import { flip } from 'svelte/animate';
  import { fade } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { activeFilterLabels, clearFilters, library } from '../lib/state.svelte';
  import PaperRow from './PaperRow.svelte';
  import Spinner from './Spinner.svelte';

  // What the empty state should blame: every non-default filter, by name.
  const activeFilters = $derived(activeFilterLabels());
</script>

<div class="min-h-0 flex-1 divide-y divide-stone-200/60 overflow-y-auto dark:divide-stone-800/60">
  {#if library.loading}
    <Spinner class="p-4" />
  {:else if library.error}
    <p class="p-4 text-sm text-red-600 dark:text-red-400">{library.error}</p>
  {:else if library.papers.length === 0}
    <div class="p-4 text-sm text-stone-500 dark:text-stone-400">
      {#if activeFilters.length > 0}
        <p>No papers match {activeFilters.join(' · ')}.</p>
        <button
          type="button"
          onclick={() => void clearFilters()}
          class="mt-2 rounded-lg border border-stone-200 px-2 py-1 text-xs hover:bg-parchment dark:border-stone-700 dark:hover:bg-stone-800"
        >Clear filters</button>
      {:else}
        <p>The library is empty — press Import to add a paper.</p>
      {/if}
    </div>
  {:else}
    {#each library.papers as paper, i (paper.id)}
      <div
        animate:flip={{ duration: dur(DUR.base) }}
        in:fade={{ duration: dur(DUR.base), delay: dur(Math.min(i * 20, 160)) }}
      >
        <PaperRow {paper} />
      </div>
    {/each}
  {/if}
</div>
