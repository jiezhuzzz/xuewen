<script lang="ts">
  import { Bookmark, ChevronRight, Ellipsis, Star } from 'lucide-svelte';
  import { onMount, tick } from 'svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';
  import NewProjectInput from './NewProjectInput.svelte';
  import { clickOutside } from '../lib/clickOutside';
  import { menuItems, menuNavKeydown } from '../lib/menuNav';
  import { clampMenuPosition } from '../lib/popoverPosition';
  import {
    createNewProject,
    deleteTag,
    filters,
    loadTags,
    projects,
    removeProject,
    renameProject,
    renameTag,
    setProjectFilter,
    setSortFilter,
    setStarFilter,
    setStatusFilter,
    setTagFilter,
    tags,
  } from '../lib/state.svelte';
  import type { Sort, StatusFilter } from '../lib/types';

  function onStatus(e: Event) {
    void setStatusFilter((e.currentTarget as HTMLSelectElement).value as StatusFilter);
  }
  function onSort(e: Event) {
    void setSortFilter((e.currentTarget as HTMLSelectElement).value as Sort);
  }

  const selectClasses =
    'min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1.5 text-xs dark:border-stone-700 dark:bg-stone-800';

  const zoneLabelClasses =
    'flex w-full items-center gap-1 text-chip font-semibold uppercase tracking-wide text-stone-400 hover:text-stone-500 dark:hover:text-stone-300';

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

  // Shared pill shape; the three kinds differ only in their color pairs.
  const pillBase = 'inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium';
  function projectPillClasses(active: boolean): string {
    return `${pillBase} ${
      active
        ? 'border-indigo-600 bg-indigo-600 text-white dark:border-indigo-500 dark:bg-indigo-500'
        : 'border-indigo-600/25 bg-indigo-600/10 text-indigo-800 hover:border-indigo-600/45 dark:border-indigo-400/25 dark:bg-indigo-400/10 dark:text-indigo-300'
    }`;
  }
  function starPillClasses(active: boolean): string {
    return `${pillBase} ${
      active
        ? 'border-orange-600/50 bg-orange-600/15 text-orange-700 dark:border-orange-400/50 dark:bg-orange-400/15 dark:text-orange-400'
        : 'border-stone-200 text-orange-700/70 hover:border-orange-600/35 dark:border-stone-700 dark:text-orange-400/70'
    }`;
  }
  function tagPillClasses(active: boolean): string {
    return `${pillBase} ${
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

  // --- per-pill context menu (rename / delete) ---
  // Opens on right-click on the pill itself, mirroring the paper rows'
  // PaperContextMenu: one cursor-anchored, viewport-clamped instance.
  // (Task 14's hover "⋯" sibling reserved layout space next to every pill —
  // opacity-0 keeps the box — which read as `pill … pill …` gaps.) macOS has
  // no Menu key and Shift+F10 doesn't reliably fire contextmenu, so each
  // pill also keeps an "⋯" trigger that is sr-only until keyboard-focused —
  // mouse users never see it and the bar stays gap-free.
  type PillKind = 'project' | 'tag';
  let openMenu = $state<{ kind: PillKind; id: string; name: string } | null>(null);
  let menuMode = $state<'menu' | 'rename' | 'delete'>('menu');
  let renameValue = $state('');
  let renameInput = $state<HTMLInputElement | null>(null);
  let menuBusy = $state(false);
  let menuError = $state<string | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);
  let menuX = 0;
  let menuY = 0;
  let left = $state(0);
  let top = $state(0);

  function openPillMenu(e: MouseEvent, kind: PillKind, id: string, name: string) {
    e.preventDefault();
    // Keyboard invocation reports (0,0) in some browsers — anchor to the
    // pill itself then.
    const pill = e.currentTarget instanceof HTMLElement ? e.currentTarget : null;
    if (e.clientX === 0 && e.clientY === 0 && pill) {
      const r = pill.getBoundingClientRect();
      menuX = r.left;
      menuY = r.bottom;
    } else {
      menuX = e.clientX;
      menuY = e.clientY;
    }
    openMenu = { kind, id, name };
    menuMode = 'menu';
    menuBusy = false;
    menuError = null;
  }
  function closeMenu() {
    openMenu = null;
    menuMode = 'menu';
    menuError = null;
  }

  // Focus moves onto the first action on open (WAI menu pattern) and back to
  // wherever it was on close — right-click doesn't move DOM focus by itself,
  // so without this, Escape/arrows would land on the page underneath.
  let prevFocus: HTMLElement | null = null;
  $effect(() => {
    if (openMenu) {
      prevFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      void tick().then(() => menuItems(menuEl)[0]?.focus());
    } else {
      prevFocus?.focus();
      prevFocus = null;
    }
  });

  // Rename focuses its input; the delete confirm focuses its first button so
  // Enter-ing "Delete" flows straight into confirm-or-cancel by keyboard.
  $effect(() => {
    if (menuMode === 'rename') renameInput?.focus();
    else if (menuMode === 'delete') menuEl?.querySelector<HTMLElement>('button')?.focus();
  });

  // Re-runs when the menu resizes (mode switch).
  $effect(() => {
    if (!openMenu || !menuEl) return;
    menuMode; // re-clamp when rename/delete changes the menu's height
    const p = clampMenuPosition(menuX, menuY, menuEl);
    left = p.left;
    top = p.top;
  });

  // Escape steps back one level (delete-confirm/rename → action list →
  // closed). The rename input handles its own Escape and stops propagation,
  // so this only sees it from the action list and the delete confirm.
  function onWindowKeydown(e: KeyboardEvent) {
    if (!openMenu || e.key !== 'Escape') return;
    if (menuMode === 'menu') closeMenu();
    else cancelRename();
  }
  function onMenuKeydown(e: KeyboardEvent) {
    if (menuMode === 'menu') menuNavKeydown(menuEl, e);
  }
  function isMenuOpen(kind: PillKind, id: string): boolean {
    return openMenu?.kind === kind && openMenu.id === id;
  }

  // A keyboard Enter/Space "click" has no coordinates, so openPillMenu's
  // (0,0) fallback anchors the menu to this trigger's own rect.
  const kbdTriggerClasses =
    'sr-only rounded-full text-stone-400 focus-visible:not-sr-only dark:text-stone-500';

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
    } finally {
      menuBusy = false;
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

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
    <option value="name">Name A–Z</option>
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
      <span class="rounded-full bg-indigo-600/15 px-1.5 text-chip tabular-nums text-indigo-700 dark:text-indigo-300">
        {projectFilterCount}
      </span>
    {/if}
  </button>
  {#if projectsOpen}
  <div class="mt-1 flex flex-wrap items-center gap-1.5">
    {#each projects.items as p (p.id)}
      <button
        type="button"
        aria-pressed={filters.project === p.id}
        onclick={() => onProjectPill(p.id)}
        oncontextmenu={(e) => openPillMenu(e, 'project', p.id, p.name)}
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
        onclick={(e) => openPillMenu(e, 'project', p.id, p.name)}
        class={kbdTriggerClasses}
      >
        <Ellipsis size={12} />
      </button>
    {/each}
    <NewProjectInput
      onCreate={(name) => createNewProject(name)}
      inputClass="w-28 rounded-full border border-dashed border-indigo-600/40 bg-paper px-2 py-0.5 text-xs outline-none focus:border-indigo-600 dark:border-indigo-400/40 dark:bg-stone-800"
      buttonClass="inline-flex items-center rounded-full border border-dashed border-stone-300 px-2 py-0.5 text-xs text-stone-400 hover:border-stone-400 hover:text-stone-600 dark:border-stone-600 dark:text-stone-500 dark:hover:border-stone-500 dark:hover:text-stone-300"
    />
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
      <span class="rounded-full bg-amber-700/15 px-1.5 text-chip tabular-nums text-amber-800 dark:text-amber-400">
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
      <button
        type="button"
        aria-pressed={filters.tag === t.name}
        onclick={() => onTagPill(t.name)}
        oncontextmenu={(e) => openPillMenu(e, 'tag', t.id, t.name)}
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
        onclick={(e) => openPillMenu(e, 'tag', t.id, t.name)}
        class={kbdTriggerClasses}
      >
        <Ellipsis size={12} />
      </button>
    {/each}
  </div>
  {/if}
</div>

{#if openMenu}
  <!-- Dismiss on any pointerdown outside the menu. The right-click that
       opened it fires its pointerdown BEFORE the menu mounts (and with it
       the action's listener) — no immediate re-close. -->
  <div
    bind:this={menuEl}
    use:clickOutside={closeMenu}
    role="menu"
    aria-label={`${openMenu.name} options`}
    tabindex="-1"
    onkeydown={onMenuKeydown}
    class="fixed z-50 w-36 rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
    style={`left:${left}px;top:${top}px`}
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
        aria-label={`Rename ${openMenu.name}`}
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
    {:else if menuBusy}
      <span class="block px-1 py-0.5 text-xs text-stone-500 dark:text-stone-400">Deleting…</span>
    {:else}
      <p class="px-1 text-xs text-stone-600 dark:text-stone-300">Delete this {openMenu.kind}?</p>
      <div class="mt-1 flex justify-end gap-1">
        <ConfirmButtons
          confirmLabel="Delete"
          onConfirm={() => void confirmDelete()}
          onCancel={() => (menuMode = 'menu')}
        />
      </div>
    {/if}
    {#if menuError}
      <p class="mt-1 px-1 text-chip text-red-600 dark:text-red-400">{menuError}</p>
    {/if}
  </div>
{/if}
