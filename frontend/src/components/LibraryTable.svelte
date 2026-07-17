<script lang="ts">
  import { ArrowDown, ArrowUp, Bookmark, Star, Tag, Trash2, X } from 'lucide-svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';
  import PaperRowTags from './PaperRowTags.svelte';
  import StatusPill from './StatusPill.svelte';
  import { openContextMenu } from '../lib/contextMenu.svelte';
  import {
    addTagToPaper,
    addToProject,
    filters,
    library,
    loadPapers,
    openTab,
    projects,
    removePapers,
    selectPaper,
    selection,
    toggleStar,
  } from '../lib/state.svelte';
  import type { PaperSummary, Sort } from '../lib/types';

  // Multi-select for bulk actions. Lives here (not in global state): it only
  // means anything while the table is on screen, and pruning keeps it honest
  // when a filter change or delete drops papers out of the list.
  let selected = $state<string[]>([]);
  let confirmingDelete = $state(false);
  let tagDraft = $state('');
  let busy = $state(false);

  $effect(() => {
    const ids = new Set(library.papers.map((p) => p.id));
    if (selected.some((id) => !ids.has(id))) {
      selected = selected.filter((id) => ids.has(id));
    }
  });

  const allSelected = $derived(
    library.papers.length > 0 && selected.length === library.papers.length,
  );

  function toggleOne(id: string) {
    selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }
  function toggleAll() {
    selected = allSelected ? [] : library.papers.map((p) => p.id);
  }
  function clearSelection() {
    selected = [];
    confirmingDelete = false;
  }

  function setSort(s: Sort) {
    filters.sort = s;
    void loadPapers();
  }

  function authorsLine(p: PaperSummary): string {
    return p.authors.length > 2
      ? `${p.authors[0]} … ${p.authors[p.authors.length - 1]}`
      : p.authors.join(', ');
  }

  function open(p: PaperSummary) {
    openTab(p);
  }
  function onRowContextMenu(e: MouseEvent, p: PaperSummary) {
    selectPaper(p.id);
    openContextMenu(e, p);
  }

  async function run(fn: () => Promise<void>) {
    busy = true;
    try {
      await fn();
    } finally {
      busy = false;
    }
  }
  function bulkStar() {
    const targets = library.papers.filter((p) => selected.includes(p.id) && !p.starred);
    void run(async () => {
      for (const p of targets) await toggleStar(p.id);
    });
  }
  function bulkTag() {
    const name = tagDraft.trim();
    if (!name) return;
    const ids = [...selected];
    void run(async () => {
      for (const id of ids) await addTagToPaper(id, name);
      tagDraft = '';
    });
  }
  function bulkProject(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    const ids = [...selected];
    void run(async () => {
      for (const id of ids) await addToProject(id, projectId);
    });
  }
  function bulkDelete() {
    const ids = [...selected];
    clearSelection();
    void run(() => removePapers(ids));
  }

  const th =
    'px-3 py-2 text-left text-caption font-semibold uppercase tracking-[.07em] text-stone-500 dark:text-stone-400';
  const sortBtn =
    'inline-flex items-center gap-1 rounded uppercase tracking-[.07em] hover:text-ink dark:hover:text-stone-200';
  const td = 'px-3 py-2.5 align-top';
  const bulkBtn =
    'inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-stone-600 hover:bg-parchment disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<div class="flex min-h-0 min-w-0 flex-1 flex-col">
  {#if selected.length > 0}
    <div class="flex shrink-0 flex-wrap items-center gap-2 border-b border-stone-200 bg-parchment/60 px-4 py-2 dark:border-stone-800 dark:bg-stone-800/40">
      <span class="text-xs font-medium text-stone-600 dark:text-stone-300">{selected.length} selected</span>
      <button type="button" class={bulkBtn} disabled={busy} onclick={bulkStar}>
        <Star size={13} /> Star
      </button>
      <form
        class="flex items-center gap-1"
        onsubmit={(e) => {
          e.preventDefault();
          bulkTag();
        }}
      >
        <Tag size={13} class="text-stone-400" />
        <input
          bind:value={tagDraft}
          placeholder="Add tag…"
          class="w-28 rounded-lg border border-stone-200 bg-paper px-2 py-1 text-xs outline-none focus:border-amber-700 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
        />
        <button type="submit" aria-label="Apply tag" class={bulkBtn} disabled={busy || !tagDraft.trim()}>Apply tag</button>
      </form>
      <label class="flex items-center gap-1 text-xs text-stone-500 dark:text-stone-400">
        <Bookmark size={13} class="text-stone-400" />
        <select
          aria-label="Add to project"
          onchange={bulkProject}
          disabled={busy}
          class="rounded-lg border border-stone-200 bg-paper px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
        >
          <option value="">Add to project…</option>
          {#each projects.items as pr (pr.id)}
            <option value={pr.id}>{pr.name}</option>
          {/each}
        </select>
      </label>
      <span class="min-w-0 flex-1"></span>
      {#if confirmingDelete}
        <ConfirmButtons
          confirmLabel={`Delete ${selected.length}`}
          onConfirm={bulkDelete}
          onCancel={() => (confirmingDelete = false)}
        />
      {:else}
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-red-600 hover:bg-red-600/10 disabled:opacity-50 dark:border-stone-700 dark:text-red-400"
          disabled={busy}
          onclick={() => (confirmingDelete = true)}
        >
          <Trash2 size={13} /> Delete
        </button>
      {/if}
      <button type="button" aria-label="Clear selection" class={bulkBtn} onclick={clearSelection}>
        <X size={13} />
      </button>
    </div>
  {/if}

  <div class="min-h-0 flex-1 overflow-auto">
    <table class="w-full table-fixed border-collapse text-sm">
      <thead class="sticky top-0 z-10 bg-paper dark:bg-night">
        <tr class="border-b border-stone-200 dark:border-stone-800">
          <th class="w-9 px-3 py-2">
            <input
              type="checkbox"
              aria-label="Select all"
              checked={allSelected}
              onchange={toggleAll}
              class="accent-amber-700"
            />
          </th>
          <th class="w-9"></th>
          <th
            class={`${th} w-[30%]`}
            aria-sort={filters.sort === 'title' ? 'ascending' : undefined}
          >
            <button type="button" class={sortBtn} onclick={() => setSort('title')}>
              Title{#if filters.sort === 'title'}<ArrowUp size={11} />{/if}
            </button>
          </th>
          <th class={`${th} w-[16%]`}>Authors</th>
          <th class={`${th} w-[14%]`}>Venue</th>
          <th
            class={`${th} w-16`}
            aria-sort={filters.sort === 'year_desc'
              ? 'descending'
              : filters.sort === 'year_asc'
                ? 'ascending'
                : undefined}
          >
            <button
              type="button"
              class={sortBtn}
              onclick={() => setSort(filters.sort === 'year_desc' ? 'year_asc' : 'year_desc')}
            >
              Year
              {#if filters.sort === 'year_desc'}<ArrowDown size={11} />{:else if filters.sort === 'year_asc'}<ArrowUp size={11} />{/if}
            </button>
          </th>
          <th class={th}>Tags</th>
          <th
            class={`${th} w-28`}
            aria-sort={filters.sort === 'added_desc' ? 'descending' : undefined}
          >
            <button type="button" class={sortBtn} onclick={() => setSort('added_desc')}>
              Added{#if filters.sort === 'added_desc'}<ArrowDown size={11} />{/if}
            </button>
          </th>
        </tr>
      </thead>
      <tbody>
        {#each library.papers as p (p.id)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the
               row's click/contextmenu are mouse conveniences; the accessible
               controls are the nested title button, star button, and checkbox
               (same rationale as PaperRow's clickable row). -->
          <tr
            data-cursor={selection.id === p.id ? 'true' : undefined}
            onclick={() => open(p)}
            oncontextmenu={(e) => onRowContextMenu(e, p)}
            class={`cursor-pointer border-b border-stone-200/60 transition-colors hover:bg-parchment/70 dark:border-stone-800/60 dark:hover:bg-stone-800/40 ${
              selection.id === p.id ? 'bg-parchment dark:bg-stone-800/50' : ''
            }`}
          >
            <td class={td} onclick={(e) => e.stopPropagation()}>
              <input
                type="checkbox"
                aria-label={`Select ${p.title ?? p.id}`}
                checked={selected.includes(p.id)}
                onchange={() => toggleOne(p.id)}
                class="accent-amber-700"
              />
            </td>
            <td class={td}>
              <button
                type="button"
                aria-label={p.starred ? 'Unstar paper' : 'Star paper'}
                aria-pressed={p.starred}
                onclick={(e) => {
                  e.stopPropagation();
                  void toggleStar(p.id);
                }}
                class={p.starred
                  ? 'text-orange-500'
                  : 'text-stone-300 hover:text-orange-400 dark:text-stone-600'}
              >
                <Star size={14} fill={p.starred ? 'currentColor' : 'none'} />
              </button>
            </td>
            <td class={td}>
              <button
                type="button"
                onclick={(e) => {
                  e.stopPropagation();
                  open(p);
                }}
                class="text-left font-serif font-medium text-ink hover:underline dark:text-stone-100"
              >
                {p.title ?? '(untitled)'}
              </button>
              <StatusPill status={p.status} />
            </td>
            <td class={`${td} text-stone-500 dark:text-stone-400`}>
              <div class="truncate" title={p.authors.join(', ')}>{authorsLine(p)}</div>
            </td>
            <td class={`${td} text-stone-500 dark:text-stone-400`}>
              <div class="truncate" title={p.venue ?? undefined}>{p.venue ?? ''}</div>
            </td>
            <td class={`${td} tabular-nums text-stone-500 dark:text-stone-400`}>{p.year ?? ''}</td>
            <td class={td}><PaperRowTags paper={p} /></td>
            <td class={`${td} whitespace-nowrap text-stone-400 dark:text-stone-500`}>
              {p.added_at
                ? new Date(p.added_at).toLocaleDateString(undefined, {
                    year: 'numeric',
                    month: 'short',
                    day: 'numeric',
                  })
                : ''}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
