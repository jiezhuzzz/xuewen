<script lang="ts">
  import { Library, Monitor, Moon, PanelLeft, Sun, Upload } from 'lucide-svelte';
  import { openImport, stats, theme, toggleSidebar, toggleTheme } from '../lib/state.svelte';

  const themeLabel = $derived(
    theme.mode === 'light' ? 'Light' : theme.mode === 'dark' ? 'Dark' : 'System',
  );
</script>

<header class="flex h-14 shrink-0 items-center justify-between border-b border-slate-200 bg-white px-4 dark:border-slate-800 dark:bg-slate-900">
  <div class="flex items-center gap-2">
    <button
      type="button"
      onclick={toggleSidebar}
      aria-label="Toggle sidebar"
      class="rounded-lg p-2 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
    >
      <PanelLeft size={18} />
    </button>
    <Library size={20} class="text-indigo-500" />
    <span class="text-lg font-semibold tracking-tight">Xuewen</span>
  </div>
  <div class="flex items-center gap-4">
    {#if stats.value}
      <div class="hidden items-center gap-3 text-xs text-slate-500 sm:flex dark:text-slate-400">
        <span>{stats.value.total} papers</span>
        <span class="text-emerald-600 dark:text-emerald-400">{stats.value.resolved} resolved</span>
        <span class="text-amber-600 dark:text-amber-400">{stats.value.needs_review} to review</span>
      </div>
    {/if}
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700"
    >
      <Upload size={16} /> Import
    </button>
    <button
      type="button"
      onclick={toggleTheme}
      aria-label={`Theme: ${themeLabel} (click to change)`}
      title={`Theme: ${themeLabel}`}
      class="rounded-lg p-2 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
    >
      {#if theme.mode === 'light'}<Sun size={18} />{:else if theme.mode === 'dark'}<Moon size={18} />{:else}<Monitor size={18} />{/if}
    </button>
  </div>
</header>
