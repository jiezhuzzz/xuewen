<script lang="ts">
  import { Bookmark, ChevronRight, Ellipsis, Star } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';
  import {
    createNewProject,
    deleteTag,
    filters,
    loadPapers,
    loadTags,
    projects,
    removeProject,
    renameProject,
    renameTag,
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

  const zoneLabelClasses =
    'flex w-full items-center gap-1 text-[10px] font-semibold uppercase tracking-wide text-stone-400 hover:text-stone-500 dark:hover:text-stone-300';

  // Projects and Star & tags start folded — the pill bars can grow long, so
  // the sidebar opens compact and the user expands what they need. A small
  // count badge on the collapsed header keeps active filters from hiding.
  let projectsOpen = $state(false);
  let starTagsOpen = $state(false);
  const projectFilterCount = $derived(filters.project !== 'all' ? 1 : 0);
  const starTagsFilterCount = $derived(
    (filters.starred === true ? 1 : 0) + (filters.tag ? 1 : 0),
  );

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

  // --- per-pill "⋯" menu (rename / delete) ---
  // Only one pill's menu is ever open at a time. The pill itself is a
  // `<button>` (the filter toggle); the "⋯" trigger and this menu are
  // SIBLINGS of that button in a wrapping <div>, never nested inside it
  // (nesting interactive elements broke a11y in Task 12).
  type PillKind = 'project' | 'tag';
  let openMenu = $state<{ kind: PillKind; id: string; name: string } | null>(null);
  let menuMode = $state<'menu' | 'rename' | 'delete'>('menu');
  let renameValue = $state('');
  let renameInput = $state<HTMLInputElement | null>(null);
  let menuBusy = $state(false);
  let menuError = $state<string | null>(null);

  function pillWrapKey(kind: PillKind, id: string): string {
    return `${kind}:${id}`;
  }
  function isMenuOpen(kind: PillKind, id: string): boolean {
    return openMenu?.kind === kind && openMenu.id === id;
  }
  function toggleMenu(kind: PillKind, id: string, name: string) {
    if (isMenuOpen(kind, id)) {
      closeMenu();
    } else {
      openMenu = { kind, id, name };
      menuMode = 'menu';
      menuError = null;
    }
  }
  function closeMenu() {
    openMenu = null;
    menuMode = 'menu';
    menuError = null;
  }
  // Click-outside-to-close: a pointerdown that lands inside the currently
  // open pill's own wrapper (the trigger, the menu, the rename input, the
  // delete confirmation) is left alone; anything else closes the menu.
  function onWindowPointerDown(e: PointerEvent) {
    if (!openMenu) return;
    if (e.target instanceof Element) {
      const wrap = e.target.closest('[data-pill-wrap]');
      if (wrap?.getAttribute('data-pill-wrap') === pillWrapKey(openMenu.kind, openMenu.id)) return;
    }
    closeMenu();
  }
  // Escape steps back one level (delete-confirm/rename -> menu list -> closed).
  // Lives on the plain wrapper div (mirroring PdfToolbar's zoom menu), not on
  // the role="menu" popover itself — putting a keydown handler directly on an
  // element with an interactive ARIA role trips svelte-check's
  // a11y_interactive_supports_focus (it wants a tabindex on the role owner).
  function onPillWrapKeydown(kind: PillKind, id: string, e: KeyboardEvent) {
    if (!isMenuOpen(kind, id) || e.key !== 'Escape') return;
    e.stopPropagation();
    if (menuMode === 'menu') closeMenu();
    else cancelRename();
  }

  $effect(() => {
    if (menuMode === 'rename') renameInput?.focus();
  });

  function startRename() {
    if (!openMenu) return;
    renameValue = openMenu.name;
    menuMode = 'rename';
    menuError = null;
  }
  function cancelRename() {
    menuMode = 'menu';
    menuError = null;
  }
  async function submitRename() {
    if (!openMenu) return;
    const name = renameValue.trim();
    if (!name || name === openMenu.name) {
      closeMenu();
      return;
    }
    menuBusy = true;
    menuError = null;
    try {
      if (openMenu.kind === 'project') {
        await renameProject(openMenu.id, { name });
      } else {
        await renameTag(openMenu.id, name);
      }
      closeMenu();
    } catch (e) {
      menuError = (e as Error).message;
    } finally {
      menuBusy = false;
    }
  }
  function onRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void submitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancelRename();
    }
  }

  function startDelete() {
    menuMode = 'delete';
    menuError = null;
  }
  async function confirmDelete() {
    if (!openMenu) return;
    menuBusy = true;
    menuError = null;
    try {
      if (openMenu.kind === 'project') {
        await removeProject(openMenu.id);
      } else {
        await deleteTag(openMenu.id);
      }
      closeMenu();
    } catch (e) {
      menuError = (e as Error).message;
      menuBusy = false;
    }
  }

  const pillMenuTriggerClasses = (open: boolean) =>
    `rounded-full p-0.5 text-stone-400 opacity-0 hover:bg-stone-200/60 hover:text-stone-700 focus-visible:opacity-100 group-hover:opacity-100 dark:text-stone-500 dark:hover:bg-stone-700/60 dark:hover:text-stone-200 ${
      open ? 'opacity-100' : ''
    }`;
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

{#snippet pillMenu(name: string, kindLabel: string)}
  <div
    role="menu"
    aria-label={`${name} options`}
    class="absolute left-0 top-full z-30 mt-1 w-36 rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
  >
    {#if menuMode === 'menu'}
      <button
        type="button"
        role="menuitem"
        onclick={startRename}
        class="block w-full rounded-lg px-2 py-1 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800"
      >
        Rename
      </button>
      <button
        type="button"
        role="menuitem"
        onclick={startDelete}
        class="block w-full rounded-lg px-2 py-1 text-left text-xs text-red-600 hover:bg-red-600/10 dark:text-red-400"
      >
        Delete
      </button>
    {:else if menuMode === 'rename'}
      <input
        bind:this={renameInput}
        bind:value={renameValue}
        type="text"
        aria-label={`Rename ${name}`}
        onkeydown={onRenameKeydown}
        class="w-full rounded-lg border border-stone-200 bg-paper px-1.5 py-1 text-xs outline-none focus:border-indigo-600 dark:border-stone-700 dark:bg-stone-800"
      />
      <div class="mt-1 flex justify-end gap-1">
        <button
          type="button"
          onclick={cancelRename}
          class="rounded-lg px-2 py-0.5 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={menuBusy}
          onclick={() => void submitRename()}
          class="rounded-lg bg-indigo-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-indigo-700 disabled:opacity-50 dark:bg-indigo-500"
        >
          Save
        </button>
      </div>
    {:else}
      <p class="px-1 text-xs text-stone-600 dark:text-stone-300">Delete this {kindLabel}?</p>
      <div class="mt-1 flex justify-end gap-1">
        <ConfirmButtons
          confirmLabel="Delete"
          onConfirm={() => void confirmDelete()}
          onCancel={() => (menuMode = 'menu')}
        />
      </div>
    {/if}
    {#if menuError}
      <p class="mt-1 px-1 text-[10px] text-red-600 dark:text-red-400">{menuError}</p>
    {/if}
  </div>
{/snippet}

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
  <button
    type="button"
    aria-expanded={projectsOpen}
    onclick={() => (projectsOpen = !projectsOpen)}
    class={zoneLabelClasses}
  >
    <ChevronRight size={11} class={`transition-transform ${projectsOpen ? 'rotate-90' : ''}`} />
    <span>Projects</span>
    {#if !projectsOpen && projectFilterCount > 0}
      <span class="rounded-full bg-indigo-600/15 px-1.5 text-[9px] tabular-nums text-indigo-700 dark:text-indigo-300">
        {projectFilterCount}
      </span>
    {/if}
  </button>
  {#if projectsOpen}
  <div class="mt-1 flex flex-wrap items-center gap-1.5">
    {#each projects.items as p (p.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions -- the keydown
           only handles Escape (see onPillWrapKeydown); every interactive
           control here is a real sibling button, never nested. -->
      <div
        class="group relative inline-flex items-center"
        data-pill-wrap={pillWrapKey('project', p.id)}
        onkeydown={(e) => onPillWrapKeydown('project', p.id, e)}
      >
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
        <button
          type="button"
          aria-label={`${p.name} options`}
          aria-haspopup="menu"
          aria-expanded={isMenuOpen('project', p.id)}
          onclick={() => toggleMenu('project', p.id, p.name)}
          class={pillMenuTriggerClasses(isMenuOpen('project', p.id))}
        >
          <Ellipsis size={12} />
        </button>
        {#if isMenuOpen('project', p.id)}
          {@render pillMenu(p.name, 'project')}
        {/if}
      </div>
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
  {/if}
</div>

<div class="mt-2">
  <button
    type="button"
    aria-expanded={starTagsOpen}
    onclick={() => (starTagsOpen = !starTagsOpen)}
    class={zoneLabelClasses}
  >
    <ChevronRight size={11} class={`transition-transform ${starTagsOpen ? 'rotate-90' : ''}`} />
    <span>Star &amp; tags</span>
    {#if !starTagsOpen && starTagsFilterCount > 0}
      <span class="rounded-full bg-amber-700/15 px-1.5 text-[9px] tabular-nums text-amber-800 dark:text-amber-400">
        {starTagsFilterCount}
      </span>
    {/if}
  </button>
  {#if starTagsOpen}
  <div class="mt-1 flex flex-wrap items-center gap-1.5">
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
      <!-- svelte-ignore a11y_no_static_element_interactions -- the keydown
           only handles Escape (see onPillWrapKeydown); every interactive
           control here is a real sibling button, never nested. -->
      <div
        class="group relative inline-flex items-center"
        data-pill-wrap={pillWrapKey('tag', t.id)}
        onkeydown={(e) => onPillWrapKeydown('tag', t.id, e)}
      >
        <button
          type="button"
          aria-pressed={filters.tag === t.name}
          onclick={() => onTagPill(t.name)}
          class={tagPillClasses(filters.tag === t.name)}
        >
          <span>{t.name}</span>
          <span class="tabular-nums opacity-70">{t.paper_count}</span>
        </button>
        <button
          type="button"
          aria-label={`${t.name} options`}
          aria-haspopup="menu"
          aria-expanded={isMenuOpen('tag', t.id)}
          onclick={() => toggleMenu('tag', t.id, t.name)}
          class={pillMenuTriggerClasses(isMenuOpen('tag', t.id))}
        >
          <Ellipsis size={12} />
        </button>
        {#if isMenuOpen('tag', t.id)}
          {@render pillMenu(t.name, 'tag')}
        {/if}
      </div>
    {/each}
  </div>
  {/if}
</div>
