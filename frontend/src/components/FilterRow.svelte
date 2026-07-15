<script lang="ts">
  import { Bookmark, Star } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import {
    createNewProject,
    filters,
    loadPapers,
    loadTags,
    projects,
    setProjectFilter,
    setStarFilter,
    setTagFilter,
    tags,
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

  const selectClasses =
    'min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-xs dark:border-stone-700 dark:bg-stone-800';

  const zoneLabelClasses = 'mb-1 text-[10px] font-semibold uppercase tracking-wide text-stone-400';

  // Nothing else populates the tags store at startup (unlike `projects`,
  // which App.svelte loads on mount) — it's otherwise only refreshed as a
  // side effect of adding/removing a tag on a paper.
  onMount(() => {
    void loadTags();
  });

  function projectPillClasses(active: boolean): string {
    return `inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium ${
      active
        ? 'border-indigo-600 bg-indigo-600 text-white dark:border-indigo-500 dark:bg-indigo-500'
        : 'border-indigo-600/25 bg-indigo-600/10 text-indigo-800 hover:border-indigo-600/45 dark:border-indigo-400/25 dark:bg-indigo-400/10 dark:text-indigo-300'
    }`;
  }
  function starPillClasses(active: boolean): string {
    return `inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium ${
      active
        ? 'border-orange-600/50 bg-orange-600/15 text-orange-700 dark:border-orange-400/50 dark:bg-orange-400/15 dark:text-orange-400'
        : 'border-stone-200 text-orange-700/70 hover:border-orange-600/35 dark:border-stone-700 dark:text-orange-400/70'
    }`;
  }
  function tagPillClasses(active: boolean): string {
    return `inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium ${
      active
        ? 'border-amber-700/40 bg-amber-700/10 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
        : 'border-stone-200 text-stone-500 hover:border-stone-300 dark:border-stone-700 dark:text-stone-400'
    }`;
  }

  function onProjectPill(id: string) {
    void setProjectFilter(filters.project === id ? 'all' : id);
  }
  function onStarPill() {
    void setStarFilter(filters.starred !== true);
  }
  function onTagPill(name: string) {
    void setTagFilter(filters.tag === name ? undefined : name);
  }

  let addingProject = $state(false);
  let newProjectName = $state('');
  let newProjectInput = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (addingProject) newProjectInput?.focus();
  });

  function startNewProject() {
    newProjectName = '';
    addingProject = true;
  }
  function cancelNewProject() {
    addingProject = false;
    newProjectName = '';
  }
  async function submitNewProject() {
    const name = newProjectName.trim();
    if (!name) {
      cancelNewProject();
      return;
    }
    cancelNewProject();
    await createNewProject(name);
  }
  function onNewProjectKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void submitNewProject();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelNewProject();
    }
  }
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

<div class="mt-2">
  <p class={zoneLabelClasses}>Projects</p>
  <div class="flex flex-wrap items-center gap-1.5">
    {#each projects.items as p (p.id)}
      <button
        type="button"
        aria-pressed={filters.project === p.id}
        onclick={() => onProjectPill(p.id)}
        class={projectPillClasses(filters.project === p.id)}
      >
        <Bookmark size={11} />
        <span>{p.name}</span>
        <span class="tabular-nums opacity-70">{p.paper_count}</span>
      </button>
    {/each}
    {#if addingProject}
      <input
        bind:this={newProjectInput}
        bind:value={newProjectName}
        type="text"
        aria-label="New project name"
        placeholder="Project name"
        onkeydown={onNewProjectKeydown}
        onblur={() => (newProjectName.trim() ? void submitNewProject() : cancelNewProject())}
        class="w-28 rounded-full border border-dashed border-indigo-600/40 bg-paper px-2 py-0.5 text-xs outline-none focus:border-indigo-600 dark:border-indigo-400/40 dark:bg-stone-800"
      />
    {:else}
      <button
        type="button"
        onclick={startNewProject}
        class="inline-flex items-center rounded-full border border-dashed border-stone-300 px-2 py-0.5 text-xs text-stone-400 hover:border-stone-400 hover:text-stone-600 dark:border-stone-600 dark:text-stone-500 dark:hover:border-stone-500 dark:hover:text-stone-300"
      >
        + New project
      </button>
    {/if}
  </div>
</div>

<div class="mt-2">
  <p class={zoneLabelClasses}>Star &amp; tags</p>
  <div class="flex flex-wrap items-center gap-1.5">
    <button
      type="button"
      aria-pressed={filters.starred === true}
      onclick={onStarPill}
      class={starPillClasses(filters.starred === true)}
    >
      <Star size={11} fill="currentColor" />
      <span>Starred</span>
    </button>
    {#each tags.items as t (t.id)}
      <button
        type="button"
        aria-pressed={filters.tag === t.name}
        onclick={() => onTagPill(t.name)}
        class={tagPillClasses(filters.tag === t.name)}
      >
        <span>{t.name}</span>
        <span class="tabular-nums opacity-70">{t.paper_count}</span>
      </button>
    {/each}
  </div>
</div>
