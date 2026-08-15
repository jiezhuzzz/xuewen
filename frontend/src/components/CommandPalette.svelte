<script lang="ts">
  import { ArrowRight, FileText, Search } from 'lucide-svelte';
  import { fade, fly } from 'svelte/transition';
  import { listPapers } from '../lib/api';
  import { fuzzyScore } from '../lib/fuzzy';
  import { SINGLE_KEYS } from '../lib/keymap';
  import { DUR, dur } from '../lib/motion';
  import { openImport } from '../lib/importQueue.svelte';
  import { library } from '../lib/library.svelte';
  import { goHome, openTab, toggleZen } from '../lib/tabs.svelte';
  import { toggleTheme } from '../lib/theme.svelte';
  import { toggleSidebar, ui } from '../lib/ui.svelte';
  import type { PaperSummary } from '../lib/types';

  let query = $state('');
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    input?.focus();
  });

  // The palette searches the WHOLE library: `library.papers` is only the
  // sidebar's current view (a filter or search replaces it), and a ⌘K jump
  // must reach papers outside that subset. Fetched fresh per open (the
  // palette is `{#if}`-gated, so mount == open, and opens are rare); until
  // it resolves — or if it fails — the filtered view stands in.
  let corpus = $state<PaperSummary[] | null>(null);
  $effect(() => {
    void listPapers({ q: '', status: 'all', sort: 'added_desc', project: 'all' })
      .then((papers) => (corpus = papers))
      .catch(() => {});
  });

  function close() {
    ui.paletteOpen = false;
    query = '';
  }

  type Item =
    | { kind: 'paper'; id: string; label: string; paper: PaperSummary; score: number }
    | { kind: 'action'; id: string; label: string; keys?: string; run: () => void; score: number };

  // The key hints come from the single-key table in lib/keymap.ts (the same
  // one the dispatcher runs), so the palette doubles as shortcut discovery
  // without keeping its own copy of the map: an unbound key loses its hint.
  const keyHint = (key: string): string | undefined =>
    SINGLE_KEYS.some((b) => b.key === key) ? key : undefined;

  const ACTIONS: Array<{ id: string; label: string; keys?: string; run: () => void }> = [
    { id: 'import', label: 'Import papers…', run: () => openImport() },
    { id: 'home', label: 'Go to library', run: () => goHome() },
    { id: 'theme', label: 'Cycle theme', run: () => toggleTheme() },
    { id: 'pane', label: 'Toggle list pane', keys: keyHint('['), run: () => toggleSidebar() },
    { id: 'zen', label: 'Toggle zen mode', keys: keyHint('z'), run: () => toggleZen() },
    { id: 'help', label: 'Keyboard shortcuts…', keys: keyHint('?'), run: () => (ui.helpOpen = true) },
  ];

  const items = $derived.by((): Item[] => {
    const q = query.trim();
    const papers: Item[] = (corpus ?? library.papers)
      .map((p) => ({
        p,
        score: fuzzyScore(q, `${p.name ?? ''} ${p.title ?? ''} ${p.authors.join(' ')} ${p.cite_key ?? ''}`),
      }))
      .filter((x): x is { p: PaperSummary; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score)
      .slice(0, 8)
      .map(({ p, score }) => ({
        kind: 'paper' as const,
        id: `paper-${p.id}`,
        label: p.title ?? '(untitled)',
        paper: p,
        score,
      }));
    const actions: Item[] = ACTIONS.map((a) => ({
      kind: 'action' as const,
      ...a,
      score: fuzzyScore(q, a.label) ?? -1,
    })).filter((a) => a.score >= 0);
    // With no query: actions first (verbs), then recent papers. With a
    // query: best matches first regardless of kind.
    return q ? [...papers, ...actions].sort((a, b) => b.score - a.score) : [...actions, ...papers];
  });

  $effect(() => {
    void items;
    active = 0;
  });

  function run(item: Item) {
    close();
    if (item.kind === 'paper') openTab(item.paper);
    else item.run();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    } else if (e.key === 'Tab') {
      // Single-focus surface: options are tabindex="-1" and the page
      // beneath is not inert, so Tab must not carry focus out.
      e.preventDefault();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.min(items.length - 1, active + 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(0, active - 1);
    } else if (e.key === 'Enter' && items[active]) {
      e.preventDefault();
      run(items[active]);
    }
  }
</script>

<div
  transition:fade={{ duration: dur(DUR.fast) }}
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
  class="fixed inset-0 z-[60] flex items-start justify-center bg-stone-950/40 p-4 pt-[12vh] backdrop-blur-[2px]"
>
  <div
    transition:fly={{ y: -12, duration: dur(DUR.base) }}
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    class="w-full max-w-lg overflow-hidden rounded-xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
  >
    <div class="flex items-center gap-2 border-b border-stone-200 px-3 dark:border-stone-800">
      <Search size={16} class="shrink-0 text-stone-400" />
      <input
        bind:this={input}
        bind:value={query}
        onkeydown={onkeydown}
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-label="Search papers and actions"
        aria-activedescendant={items[active] ? `palette-opt-${active}` : undefined}
        placeholder="Type a paper title or a command…"
        class="w-full bg-transparent py-3 text-sm text-ink outline-none dark:text-stone-100"
      />
    </div>
    <ul id="palette-list" role="listbox" class="max-h-80 overflow-y-auto p-1">
      {#if items.length === 0}
        <li class="px-3 py-4 text-sm text-stone-500 dark:text-stone-400">
          Nothing matches. Try fewer letters.
        </li>
      {/if}
      {#each items as item, i (item.id)}
        <li id={`palette-opt-${i}`} role="option" aria-selected={i === active}>
          <button
            type="button"
            tabindex="-1"
            onclick={() => run(item)}
            onmouseenter={() => (active = i)}
            class={`flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm ${
              i === active
                ? 'bg-amber-700/10 text-ink dark:bg-amber-500/10 dark:text-stone-100'
                : 'text-stone-600 dark:text-stone-300'
            }`}
          >
            {#if item.kind === 'paper'}
              <FileText size={14} class="shrink-0 text-stone-400" />
              <span class="min-w-0 flex-1 truncate font-serif">{item.label}</span>
            {:else}
              <ArrowRight size={14} class="shrink-0 text-stone-400" />
              <span class="min-w-0 flex-1 truncate">{item.label}</span>
              {#if item.keys}
                <kbd class="shrink-0 rounded border border-stone-300 px-1 font-mono text-caption text-stone-400 dark:border-stone-700 dark:text-stone-500">{item.keys}</kbd>
              {/if}
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </div>
</div>
