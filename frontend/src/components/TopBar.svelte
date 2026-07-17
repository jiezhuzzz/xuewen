<script lang="ts">
  import { Monitor, Moon, PanelLeft, Sun, Upload } from 'lucide-svelte';
  import {
    filters,
    library,
    loadPapers,
    openImport,
    stats,
    theme,
    toggleSidebar,
    toggleTheme,
    ui,
  } from '../lib/state.svelte';
  import SealMark from './SealMark.svelte';

  const themeLabel = $derived(
    theme.mode === 'light' ? 'Light' : theme.mode === 'dark' ? 'Dark' : 'System',
  );

  // While searching, library.papers holds relevance-ranked search results
  // rather than the whole library — show how many matched instead of the
  // library-wide total.
  const searching = $derived(filters.q.trim() !== '');
  const matchCount = $derived(library.papers.length);
</script>

<header class="flex h-14 shrink-0 items-center justify-between border-b border-stone-200 bg-paper px-4 dark:border-stone-800 dark:bg-night">
  <div class="flex items-center gap-2">
    <button
      type="button"
      onclick={toggleSidebar}
      aria-label="Toggle list pane"
      title="Toggle list pane ([)"
      class="rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <PanelLeft size={18} />
    </button>
    <SealMark size={22} />
    <span class="font-serif text-lg font-semibold tracking-tight">Xuewen</span>
  </div>
  <div class="flex items-center gap-3">
    {#if stats.value}
      <div class="hidden items-center gap-3 text-xs text-stone-500 sm:flex dark:text-stone-400">
        {#if searching}
          <span>{matchCount} {matchCount === 1 ? 'match' : 'matches'}</span>
        {:else}
          <span>{stats.value.total} papers</span>
        {/if}
        {#if stats.value.needs_review > 0}
          <button
            type="button"
            title="Show papers that need review"
            onclick={() => {
              filters.status = 'needs_review';
              void loadPapers();
            }}
            class="rounded text-yellow-700 underline-offset-2 hover:underline dark:text-yellow-400"
          >{stats.value.needs_review} to review</button>
        {/if}
      </div>
    {/if}
    <button
      type="button"
      onclick={() => (ui.paletteOpen = true)}
      class="hidden items-center gap-1 rounded-lg border border-stone-200 px-2 py-1 text-xs text-stone-400 hover:bg-parchment sm:inline-flex dark:border-stone-700 dark:hover:bg-stone-800"
    >
      <kbd>⌘K</kbd>
    </button>
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      <Upload size={16} /> Import
    </button>
    <button
      type="button"
      onclick={toggleTheme}
      aria-label={`Theme: ${themeLabel} (click to change)`}
      title={`Theme: ${themeLabel}`}
      class="rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      {#if theme.mode === 'light'}<Sun size={18} />{:else if theme.mode === 'dark'}<Moon size={18} />{:else}<Monitor size={18} />{/if}
    </button>
  </div>
</header>
