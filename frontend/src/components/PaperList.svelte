<script lang="ts">
  import { flip } from 'svelte/animate';
  import { fade } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { library } from '../lib/state.svelte';
  import PaperRow from './PaperRow.svelte';
  import Spinner from './Spinner.svelte';
</script>

<div class="min-h-0 flex-1 divide-y divide-stone-200/60 overflow-y-auto dark:divide-stone-800/60">
  {#if library.loading}
    <Spinner class="p-4" />
  {:else if library.error}
    <p class="p-4 text-sm text-red-600 dark:text-red-400">{library.error}</p>
  {:else if library.papers.length === 0}
    <p class="p-4 text-sm text-stone-500 dark:text-stone-400">
      No papers match. Clear the search or import one.
    </p>
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
