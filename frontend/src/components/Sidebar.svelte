<script lang="ts">
  import { Search } from 'lucide-svelte';
  import { filters, library, loadPapers, setSearch } from '../lib/state.svelte';
  import type { Sort, StatusFilter } from '../lib/types';
  import PaperRow from './PaperRow.svelte';

  function onStatus(e: Event) {
    filters.status = (e.currentTarget as HTMLSelectElement).value as StatusFilter;
    loadPapers();
  }
  function onSort(e: Event) {
    filters.sort = (e.currentTarget as HTMLSelectElement).value as Sort;
    loadPapers();
  }
</script>

<aside class="flex h-full w-80 shrink-0 flex-col border-r border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
  <div class="space-y-3 border-b border-slate-200 p-3 dark:border-slate-800">
    <div class="relative">
      <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-slate-500 dark:text-slate-400" />
      <input
        type="search"
        aria-label="Search papers"
        placeholder="Search title or author…"
        value={filters.q}
        oninput={(e) => setSearch((e.currentTarget as HTMLInputElement).value)}
        class="w-full rounded-lg border border-slate-200 bg-slate-50 py-2 pl-9 pr-3 text-sm outline-none focus:border-indigo-400 focus:ring-2 focus:ring-indigo-500/20 dark:border-slate-700 dark:bg-slate-800"
      />
    </div>
    <div class="flex gap-2">
      <select
        value={filters.status}
        aria-label="Filter by status"
        onchange={onStatus}
        class="flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs dark:border-slate-700 dark:bg-slate-800"
      >
        <option value="all">All status</option>
        <option value="resolved">Resolved</option>
        <option value="needs_review">Needs review</option>
      </select>
      <select
        value={filters.sort}
        aria-label="Sort papers"
        onchange={onSort}
        class="flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs dark:border-slate-700 dark:bg-slate-800"
      >
        <option value="year_desc">Newest</option>
        <option value="year_asc">Oldest</option>
        <option value="added_desc">Recently added</option>
        <option value="title">Title A–Z</option>
      </select>
    </div>
  </div>

  <div class="min-h-0 flex-1 divide-y divide-slate-100 overflow-y-auto dark:divide-slate-800/60">
    {#if library.loading}
      <p class="p-4 text-sm text-slate-500 dark:text-slate-400">Loading…</p>
    {:else if library.error}
      <p class="p-4 text-sm text-red-600 dark:text-red-400">{library.error}</p>
    {:else if library.papers.length === 0}
      <p class="p-4 text-sm text-slate-500 dark:text-slate-400">No papers match.</p>
    {:else}
      {#each library.papers as paper (paper.id)}
        <PaperRow {paper} />
      {/each}
    {/if}
  </div>
</aside>
