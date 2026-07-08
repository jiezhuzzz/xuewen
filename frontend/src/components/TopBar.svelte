<script lang="ts">
  import { Library, Moon, Sun } from 'lucide-svelte';
  import { stats, theme, toggleTheme } from '../lib/state.svelte';
</script>

<header class="flex h-14 shrink-0 items-center justify-between border-b border-slate-200 bg-white px-4 dark:border-slate-800 dark:bg-slate-900">
  <div class="flex items-center gap-2">
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
      onclick={toggleTheme}
      aria-label="Toggle theme"
      class="rounded-lg p-2 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
    >
      {#if theme.mode === 'dark'}<Sun size={18} />{:else}<Moon size={18} />{/if}
    </button>
  </div>
</header>
