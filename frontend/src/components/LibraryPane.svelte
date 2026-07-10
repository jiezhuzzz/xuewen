<script lang="ts">
  import { Download } from 'lucide-svelte';
  import { exportUrl } from '../lib/api';
  import { bibFormat, filters } from '../lib/state.svelte';
  import FilterRow from './FilterRow.svelte';
  import PaperList from './PaperList.svelte';
  import SearchBox from './SearchBox.svelte';
</script>

<aside class="flex h-full w-[304px] shrink-0 flex-col border-r border-stone-200 bg-parchment/60 dark:border-stone-800 dark:bg-soot/60">
  <div class="space-y-3 border-b border-stone-200 p-3 dark:border-stone-800">
    <SearchBox />
    <FilterRow />
  </div>

  <PaperList />

  <div class="border-t border-stone-200 p-2 dark:border-stone-800">
    {#if filters.q.trim()}
      <!-- Batch export filters by the legacy title/author match, not hybrid
           search results — hidden while a query is active to avoid exporting
           a different set than the list shows. -->
      <span
        title="Clear the search to export"
        class="inline-flex w-full cursor-not-allowed items-center justify-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1.5 text-xs font-medium text-stone-400 dark:border-stone-700 dark:text-stone-600"
      >
        <Download size={14} /> Export .bib
      </span>
    {:else}
      <a
        href={exportUrl(filters, bibFormat.value)}
        download="xuewen.bib"
        class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1.5 text-xs font-medium text-stone-600 hover:bg-parchment dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
      >
        <Download size={14} /> Export .bib
      </a>
    {/if}
  </div>
</aside>
