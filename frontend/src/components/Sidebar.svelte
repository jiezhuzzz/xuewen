<script lang="ts">
  import { Download, FolderOpen, Search, Settings2 } from 'lucide-svelte';
  import {
    bibFormat,
    filters,
    library,
    loadPapers,
    openProjects,
    projects,
    searchMeta,
    searchOpts,
    semanticBlocked,
    setProjectFilter,
    setSearch,
    toggleSearchEngine,
    toggleSearchField,
  } from '../lib/state.svelte';
  import { exportUrl } from '../lib/api';
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
  function onProject(e: Event) {
    void setProjectFilter((e.currentTarget as HTMLSelectElement).value);
  }
</script>

<aside class="flex h-full w-80 shrink-0 flex-col border-r border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
  <div class="space-y-3 border-b border-slate-200 p-3 dark:border-slate-800">
    <div class="relative">
      <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-slate-500 dark:text-slate-400" />
      <input
        type="search"
        aria-label="Search papers"
        placeholder="Search library…"
        value={filters.q}
        oninput={(e) => setSearch((e.currentTarget as HTMLInputElement).value)}
        class="w-full rounded-lg border border-slate-200 bg-slate-50 py-2 pl-9 pr-3 text-sm outline-none focus:border-indigo-400 focus:ring-2 focus:ring-indigo-500/20 dark:border-slate-700 dark:bg-slate-800"
      />
    </div>
    <div class="flex flex-wrap gap-1 text-[11px]">
      {#each [['title', 'Title'], ['authors', 'Authors'], ['abstract', 'Abstract'], ['body', 'Body']] as [key, label] (key)}
        <button
          type="button"
          aria-pressed={searchOpts[key as 'title' | 'authors' | 'abstract' | 'body']}
          onclick={() => toggleSearchField(key as 'title' | 'authors' | 'abstract' | 'body')}
          class={`rounded-full border px-2 py-0.5 ${
            searchOpts[key as 'title' | 'authors' | 'abstract' | 'body']
              ? 'border-indigo-300 bg-indigo-50 text-indigo-700 dark:border-indigo-700 dark:bg-indigo-950 dark:text-indigo-300'
              : 'border-slate-200 text-slate-400 dark:border-slate-700 dark:text-slate-500'
          }`}
        >
          {label}
        </button>
      {/each}
      <span class="mx-1 border-l border-slate-200 dark:border-slate-700"></span>
      <button
        type="button"
        aria-pressed={searchOpts.keyword}
        onclick={() => toggleSearchEngine('keyword')}
        class={`rounded-full border px-2 py-0.5 ${
          searchOpts.keyword
            ? 'border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-300'
            : 'border-slate-200 text-slate-400 dark:border-slate-700 dark:text-slate-500'
        }`}
      >
        Keyword
      </button>
      <button
        type="button"
        aria-pressed={searchOpts.semantic && !semanticBlocked()}
        disabled={semanticBlocked()}
        title={searchMeta.semantic.reason ?? undefined}
        onclick={() => toggleSearchEngine('semantic')}
        class={`rounded-full border px-2 py-0.5 disabled:cursor-not-allowed disabled:opacity-40 ${
          searchOpts.semantic && !semanticBlocked()
            ? 'border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-300'
            : 'border-slate-200 text-slate-400 dark:border-slate-700 dark:text-slate-500'
        }`}
      >
        Semantic
      </button>
    </div>
    {#if searchMeta.pending > 0}
      <p class="text-[11px] text-slate-400 dark:text-slate-500">
        indexing {searchMeta.pending} paper{searchMeta.pending === 1 ? '' : 's'}…
      </p>
    {/if}
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
    <div class="flex items-center gap-2">
      <FolderOpen size={16} class="shrink-0 text-slate-500 dark:text-slate-400" />
      <select
        value={filters.project}
        aria-label="Filter by project"
        onchange={onProject}
        class="min-w-0 flex-1 rounded-lg border border-slate-200 bg-slate-50 px-2 py-1.5 text-xs dark:border-slate-700 dark:bg-slate-800"
      >
        <option value="all">All projects</option>
        {#each projects.items as p (p.id)}
          <option value={p.id}>{p.name} ({p.paper_count})</option>
        {/each}
      </select>
      <button
        type="button"
        aria-label="Manage projects"
        onclick={openProjects}
        class="rounded-lg border border-slate-200 p-1.5 text-slate-500 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <Settings2 size={16} />
      </button>
    </div>
    <a
      href={exportUrl(filters, bibFormat.value)}
      download="xuewen.bib"
      class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg border border-slate-200 px-2 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
    >
      <Download size={14} /> Export .bib
    </a>
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
