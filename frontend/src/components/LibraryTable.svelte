<script lang="ts">
  import { ArrowDown, ArrowUp, Bookmark, Star, Tag, Trash2, X } from 'lucide-svelte';
  import ColumnHeaderMenu from './ColumnHeaderMenu.svelte';
  import ColumnResizeHandle from './ColumnResizeHandle.svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';
  import PaperRowTags from './PaperRowTags.svelte';
  import { openContextMenu } from '../lib/contextMenu.svelte';
  import {
    addPapersToProject,
    addTagToPapers,
    filters,
    library,
    openTab,
    projects,
    removePapers,
    selectPaper,
    selection,
    setSortFilter,
    toggleStar,
  } from '../lib/state.svelte';
  import { abbreviateVenue } from '../lib/venue';
  import { fitToAvailable, measureColumnFromDom } from '../lib/columnAutoFit';
  import {
    columnWidths,
    commitColumnWidth,
    commitColumnWidths,
    resetColumnWidths,
    setColumnWidth,
  } from '../lib/columnWidths.svelte';
  import {
    AUTO_FIT_PADDING,
    ICON_COLUMN_PX,
    PINNED_COLUMNS,
    PINNED_KEYS,
    TAGS_MIN_PX,
    TAGS_TARGET_PX,
    type PinnedColumnKey,
  } from '../lib/tableColumns';
  import type { PaperSummary, Sort } from '../lib/types';

  // Multi-select for bulk actions. Lives here (not in global state): it only
  // means anything while the table is on screen, and pruning keeps it honest
  // when a filter change or delete drops papers out of the list.
  let selected = $state<string[]>([]);
  let confirmingDelete = $state(false);
  let tagDraft = $state('');
  let busy = $state(false);

  // Memoized id sets: the prune effect re-runs on every selection change too,
  // and each row's checkbox re-checks membership when `selected` changes —
  // both stay O(1)-per-lookup instead of rescanning arrays.
  const paperIds = $derived(new Set(library.papers.map((p) => p.id)));
  const selectedSet = $derived(new Set(selected));

  $effect(() => {
    if (selected.some((id) => !paperIds.has(id))) {
      selected = selected.filter((id) => paperIds.has(id));
    }
  });

  const allSelected = $derived(
    library.papers.length > 0 && selected.length === library.papers.length,
  );

  function toggleOne(id: string) {
    selected = selectedSet.has(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }
  function toggleAll() {
    selected = allSelected ? [] : library.papers.map((p) => p.id);
  }
  function clearSelection() {
    selected = [];
    confirmingDelete = false;
  }

  function setSort(s: Sort) {
    void setSortFilter(s);
  }

  // While a query is active the server ranks by relevance; sort headers
  // would lie, so they go inert (arrows off, buttons disabled, no aria-sort).
  const searching = $derived(filters.q.trim() !== '');

  let tableEl = $state<HTMLTableElement | null>(null);
  let headerMenu = $state<{ x: number; y: number } | null>(null);

  // Widest `key` may grow while every other pinned column keeps its width
  // and Tags keeps its reserve — the invariant that keeps the table filling
  // the pane with no horizontal scroll. Falls back to the static max when
  // layout isn't available (jsdom).
  function maxFor(key: PinnedColumnKey): number {
    const stat = PINNED_COLUMNS[key].maxWidth;
    const container = tableEl?.clientWidth ?? 0;
    if (container <= 0) return stat;
    const others = PINNED_KEYS.reduce((s, k) => (k === key ? s : s + columnWidths[k]), 0);
    return Math.max(
      PINNED_COLUMNS[key].minWidth,
      Math.min(stat, container - 2 * ICON_COLUMN_PX - others - TAGS_MIN_PX),
    );
  }

  function autoFit(key: PinnedColumnKey) {
    if (!tableEl) return;
    const def = PINNED_COLUMNS[key];
    commitColumnWidth(
      key,
      measureColumnFromDom(tableEl, key, {
        min: def.minWidth,
        max: maxFor(key),
        padding: AUTO_FIT_PADDING,
      }),
    );
  }

  // Fit every column to its content, then scale the whole set toward the
  // pane width — down when it doesn't fit, up into surplus when it does, so
  // spare width lands in the content columns rather than pooling in Tags
  // (which keeps a comfortable TAGS_TARGET_PX strip).
  function autoFitAll() {
    if (!tableEl) return;
    const natural = {} as Record<PinnedColumnKey, number>;
    const bounds = {} as Record<PinnedColumnKey, { min: number; max: number }>;
    for (const key of PINNED_KEYS) {
      const def = PINNED_COLUMNS[key];
      natural[key] = measureColumnFromDom(tableEl, key, {
        min: def.minWidth,
        max: def.maxWidth,
        padding: AUTO_FIT_PADDING,
      });
      bounds[key] = { min: def.minWidth, max: def.maxWidth };
    }
    const container = tableEl.clientWidth;
    const fitted =
      container > 0
        ? fitToAvailable(natural, bounds, container - 2 * ICON_COLUMN_PX - TAGS_TARGET_PX)
        : natural;
    commitColumnWidths(fitted);
  }

  function onHeaderContextMenu(e: MouseEvent) {
    e.preventDefault();
    headerMenu = { x: e.clientX, y: e.clientY };
  }

  // One shared formatter: constructing Intl.DateTimeFormat per row per render
  // (what toLocaleDateString with options does) is measurably expensive.
  const DATE_FMT = new Intl.DateTimeFormat(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });

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
    const targets = library.papers.filter((p) => selectedSet.has(p.id) && !p.starred);
    void run(async () => {
      // Independent papers; toggleStar handles its own rollback/toast per id.
      await Promise.all(targets.map((p) => toggleStar(p.id)));
    });
  }
  function bulkTag() {
    const name = tagDraft.trim();
    if (!name) return;
    const ids = [...selected];
    void run(async () => {
      await addTagToPapers(ids, name);
      tagDraft = '';
    });
  }
  function bulkProject(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    const ids = [...selected];
    void run(() => addPapersToProject(ids, projectId));
  }
  function bulkDelete() {
    const ids = [...selected];
    clearSelection();
    void run(() => removePapers(ids));
  }

  const th =
    'px-3 py-1.5 text-left text-caption font-semibold uppercase tracking-[.07em] text-stone-500 dark:text-stone-400';
  const sortBtn =
    'inline-flex items-center gap-1 rounded uppercase tracking-[.07em] hover:text-ink disabled:cursor-default disabled:hover:text-inherit dark:hover:text-stone-200';
  const td = 'px-3 py-1.5 align-top';
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

  {#snippet resizeHandle(key: PinnedColumnKey, edge: 'left' | 'right' = 'right')}
    <ColumnResizeHandle
      label={PINNED_COLUMNS[key].label}
      width={columnWidths[key]}
      min={PINNED_COLUMNS[key].minWidth}
      max={() => maxFor(key)}
      {edge}
      onLiveResize={(px) => setColumnWidth(key, px)}
      onCommit={(px) => commitColumnWidth(key, px)}
      onAutoFit={() => autoFit(key)}
    />
  {/snippet}

  <div class="min-h-0 flex-1 overflow-auto">
    <table bind:this={tableEl} class="w-full table-fixed border-collapse text-sm">
      <!-- Single source of column widths. Tags has no <col> width on
           purpose: under table-fixed the one width-less column absorbs the
           remainder, which is what keeps the table exactly pane-wide. -->
      <colgroup>
        <col style={`width:${ICON_COLUMN_PX}px`} />
        <col style={`width:${ICON_COLUMN_PX}px`} />
        <col style={`width:${columnWidths.name}px`} />
        <col style={`width:${columnWidths.title}px`} />
        <col style={`width:${columnWidths.firstAuthor}px`} />
        <col style={`width:${columnWidths.lastAuthor}px`} />
        <col style={`width:${columnWidths.venue}px`} />
        <col style={`width:${columnWidths.year}px`} />
        <col />
        <col style={`width:${columnWidths.added}px`} />
      </colgroup>
      <thead class="sticky top-0 z-10 bg-paper dark:bg-night">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the
             right-click column menu is a mouse convenience; resizing stays
             keyboard-reachable through the focusable handles. -->
        <tr
          class="border-b border-stone-200 dark:border-stone-800"
          oncontextmenu={onHeaderContextMenu}
        >
          <th class="px-3 py-1.5">
            <input
              type="checkbox"
              aria-label="Select all"
              checked={allSelected}
              onchange={toggleAll}
              class="accent-amber-700"
            />
          </th>
          <th></th>
          <th
            class={`${th} relative`}
            data-col="name"
            aria-sort={!searching && filters.sort === 'name' ? 'ascending' : undefined}
          >
            <button
              type="button"
              class={sortBtn}
              disabled={searching}
              title={searching ? 'Sorted by relevance during search' : undefined}
              onclick={() => setSort('name')}
            >
              Name{#if !searching && filters.sort === 'name'}<ArrowUp size={11} />{/if}
            </button>
            {@render resizeHandle('name')}
          </th>
          <th
            class={`${th} relative`}
            data-col="title"
            aria-sort={!searching && filters.sort === 'title' ? 'ascending' : undefined}
          >
            <button
              type="button"
              class={sortBtn}
              disabled={searching}
              title={searching ? 'Sorted by relevance during search' : undefined}
              onclick={() => setSort('title')}
            >
              Title{#if !searching && filters.sort === 'title'}<ArrowUp size={11} />{/if}
            </button>
            {@render resizeHandle('title')}
          </th>
          <th class={`${th} relative`} data-col="firstAuthor">
            First author
            {@render resizeHandle('firstAuthor')}
          </th>
          <th class={`${th} relative`} data-col="lastAuthor">
            Last author
            {@render resizeHandle('lastAuthor')}
          </th>
          <th class={`${th} relative`} data-col="venue">
            Venue
            {@render resizeHandle('venue')}
          </th>
          <th
            class={`${th} relative`}
            data-col="year"
            aria-sort={!searching && filters.sort === 'year_desc'
              ? 'descending'
              : !searching && filters.sort === 'year_asc'
                ? 'ascending'
                : undefined}
          >
            <button
              type="button"
              class={sortBtn}
              disabled={searching}
              title={searching ? 'Sorted by relevance during search' : undefined}
              onclick={() => setSort(filters.sort === 'year_desc' ? 'year_asc' : 'year_desc')}
            >
              Year
              {#if !searching}{#if filters.sort === 'year_desc'}<ArrowDown size={11} />{:else if filters.sort === 'year_asc'}<ArrowUp size={11} />{/if}{/if}
            </button>
            {@render resizeHandle('year')}
          </th>
          <th class={th}>Tags</th>
          <th
            class={`${th} relative`}
            data-col="added"
            aria-sort={!searching && filters.sort === 'added_desc' ? 'descending' : undefined}
          >
            <button
              type="button"
              class={sortBtn}
              disabled={searching}
              title={searching ? 'Sorted by relevance during search' : undefined}
              onclick={() => setSort('added_desc')}
            >
              Added{#if !searching && filters.sort === 'added_desc'}<ArrowDown size={11} />{/if}
            </button>
            <!-- Left-edge: Added's right edge IS the table edge; the divider
                 it owns is the Tags|Added boundary on its left. -->
            {@render resizeHandle('added', 'left')}
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
                checked={selectedSet.has(p.id)}
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
            <td class={`${td} font-mono text-xs text-stone-600 dark:text-stone-300`}>
              <div class="truncate" data-col="name" title={p.name ?? undefined}>
                {#if p.name}{p.name}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
              </div>
            </td>
            <td class={td}>
              <button
                type="button"
                data-col="title"
                onclick={(e) => {
                  e.stopPropagation();
                  open(p);
                }}
                class="text-left font-serif font-medium text-ink hover:underline dark:text-stone-100"
              >
                {p.title ?? '(untitled)'}
              </button>
              <!-- No status pill here: the sidebar list still flags
                   needs-review papers, and right-click → Identify… is the
                   repair path from the table. -->
            </td>
            <!-- First/last author are the ordered list's ends; a single-
                 author paper repeats the name in both columns ([0] and
                 .at(-1) coincide). Tooltips keep the full roster — the only
                 place elided middle authors still show. -->
            {#snippet authorCell(col: 'firstAuthor' | 'lastAuthor')}
              {@const name = col === 'firstAuthor' ? p.authors[0] : p.authors.at(-1)}
              <td class={`${td} text-stone-500 dark:text-stone-400`}>
                <div class="truncate" data-col={col} title={p.authors.join(', ') || undefined}>
                  {#if name}{name}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
                </div>
              </td>
            {/snippet}
            {@render authorCell('firstAuthor')}
            {@render authorCell('lastAuthor')}
            <td class={`${td} text-stone-500 dark:text-stone-400`}>
              <div class="truncate" data-col="venue" title={p.venue ?? undefined}>
                {#if p.venue}{abbreviateVenue(p.venue)}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
              </div>
            </td>
            <td class={`${td} tabular-nums text-stone-500 dark:text-stone-400`} data-col="year">
              {#if p.year !== null}{p.year}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
            </td>
            <td class={td}><PaperRowTags paper={p} /></td>
            <td class={`${td} whitespace-nowrap text-stone-400 dark:text-stone-500`} data-col="added">
              {p.added_at ? DATE_FMT.format(new Date(p.added_at)) : ''}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if headerMenu}
    <ColumnHeaderMenu
      x={headerMenu.x}
      y={headerMenu.y}
      onAutoFitAll={autoFitAll}
      onReset={resetColumnWidths}
      onClose={() => (headerMenu = null)}
    />
  {/if}
</div>
