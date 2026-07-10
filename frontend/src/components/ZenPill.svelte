<script lang="ts">
  import { Minimize2 } from 'lucide-svelte';
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { ui, viewer } from '../lib/state.svelte';

  const title = $derived(viewer.tabs.find((t) => t.id === viewer.activeId)?.title ?? '');
</script>

<div
  transition:fly={{ y: -16, duration: dur(DUR.base) }}
  class="fixed left-1/2 top-3 z-50 flex max-w-md -translate-x-1/2 items-center gap-2 rounded-full border border-stone-200 bg-paper/90 px-4 py-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/90"
>
  <span class="truncate font-serif text-sm text-ink dark:text-stone-100">{title}</span>
  <button
    type="button"
    aria-label="Exit zen mode"
    title="Exit zen (Esc)"
    onclick={() => (ui.zen = false)}
    class="rounded-full p-1 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
  >
    <Minimize2 size={14} />
  </button>
</div>
