<script lang="ts">
  import { Upload } from 'lucide-svelte';
  import { anyFilterActive, library, openImport } from '../lib/state.svelte';
  import SealMark from './SealMark.svelte';
</script>

<div class="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
  <SealMark size={48} />
  <h2 class="font-serif text-2xl font-semibold text-ink dark:text-stone-100">Xuewen</h2>
  {#if library.papers.length === 0 && !anyFilterActive()}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">
      Your library is empty. Import a PDF, a DOI, or an arXiv link to begin.
    </p>
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      <Upload size={16} /> Import papers
    </button>
  {:else}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">
      Click a paper to read it. Press <kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">i</kbd> for its details.
    </p>
    <dl class="grid grid-cols-[auto_auto] gap-x-3 gap-y-1 text-xs text-stone-400 dark:text-stone-500">
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">/</kbd></dt>
      <dd class="text-left">search</dd>
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">⌘K</kbd></dt>
      <dd class="text-left">command palette</dd>
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">z</kbd></dt>
      <dd class="text-left">zen mode while reading</dd>
    </dl>
  {/if}
</div>
