<script lang="ts">
  import { FolderOpen, Settings2 } from 'lucide-svelte';
  import {
    filters,
    loadPapers,
    openProjects,
    projects,
    setProjectFilter,
  } from '../lib/state.svelte';
  import type { Sort, StatusFilter } from '../lib/types';

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

  const selectClasses =
    'min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-xs dark:border-stone-700 dark:bg-stone-800';
</script>

<div class="flex gap-2">
  <select value={filters.status} aria-label="Filter by status" onchange={onStatus} class={selectClasses}>
    <option value="all">All status</option>
    <option value="resolved">Resolved</option>
    <option value="needs_review">Needs review</option>
  </select>
  <select value={filters.sort} aria-label="Sort papers" onchange={onSort} class={selectClasses}>
    <option value="year_desc">Newest</option>
    <option value="year_asc">Oldest</option>
    <option value="added_desc">Recently added</option>
    <option value="title">Title A–Z</option>
  </select>
</div>
<div class="mt-2 flex items-center gap-2">
  <FolderOpen size={16} class="shrink-0 text-stone-500 dark:text-stone-400" />
  <select value={filters.project} aria-label="Filter by project" onchange={onProject} class={selectClasses}>
    <option value="all">All projects</option>
    {#each projects.items as p (p.id)}
      <option value={p.id}>{p.name} ({p.paper_count})</option>
    {/each}
  </select>
  <button
    type="button"
    aria-label="Manage projects"
    onclick={openProjects}
    class="rounded-lg border border-stone-200 p-1.5 text-stone-500 hover:bg-parchment dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800"
  >
    <Settings2 size={16} />
  </button>
</div>
