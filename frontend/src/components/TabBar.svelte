<script lang="ts">
  import { Info, LibraryBig, Maximize2, X } from 'lucide-svelte';
  import { flip } from 'svelte/animate';
  import { crossfade, fade } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { closeTab, goHome, toggleZen, viewer } from '../lib/state.svelte';

  // The active-tab underline crossfades between tabs — a real sliding
  // indicator with no measurement code.
  const [send, receive] = crossfade({ duration: dur(DUR.fast) });
</script>

<div class="flex h-11 shrink-0 items-center border-b border-stone-200 bg-paper dark:border-stone-800 dark:bg-night">
  <button
    type="button"
    aria-label="Library"
    aria-current={viewer.activeId === null ? 'page' : undefined}
    onclick={goHome}
    class={`relative flex h-11 shrink-0 items-center gap-1.5 px-3 text-sm ${
      viewer.activeId === null
        ? 'text-ink dark:text-stone-100'
        : 'text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800/40'
    }`}
  >
    <LibraryBig size={15} />
    Library
    {#if viewer.activeId === null}
      <span
        in:receive={{ key: 'tab-underline' }}
        out:send={{ key: 'tab-underline' }}
        class="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-amber-700 dark:bg-amber-500"
      ></span>
    {/if}
  </button>
  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <div class="flex min-w-0 flex-1 items-center overflow-x-auto">
    {#each viewer.tabs as tab (tab.id)}
      <div
        animate:flip={{ duration: dur(DUR.base) }}
        out:fade={{ duration: dur(DUR.fast) }}
        class={`group relative flex h-11 max-w-52 shrink-0 items-center gap-2 border-r border-stone-200 px-3 dark:border-stone-800 ${
          viewer.activeId === tab.id
            ? 'bg-parchment/70 dark:bg-stone-800/60'
            : 'hover:bg-parchment/50 dark:hover:bg-stone-800/30'
        }`}
      >
        <button
          type="button"
          onclick={() => (viewer.activeId = tab.id)}
          class="min-w-0 truncate font-serif text-sm text-stone-700 dark:text-stone-200"
        >
          {tab.title}
        </button>
        <button
          type="button"
          aria-label="Close tab"
          onclick={() => closeTab(tab.id)}
          class="rounded p-0.5 text-stone-500 opacity-0 hover:bg-stone-200 group-hover:opacity-100 dark:text-stone-400 dark:hover:bg-stone-700"
        >
          <X size={14} />
        </button>
        {#if viewer.activeId === tab.id}
          <span
            in:receive={{ key: 'tab-underline' }}
            out:send={{ key: 'tab-underline' }}
            class="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-amber-700 dark:bg-amber-500"
          ></span>
        {/if}
      </div>
    {/each}
  </div>

  {#if viewer.activeId !== null}
    <button
      type="button"
      aria-label="Zen mode"
      title="Zen mode (z)"
      onclick={toggleZen}
      class="mr-1 shrink-0 rounded-lg p-2 text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
    >
      <Maximize2 size={16} />
    </button>
    <button
      type="button"
      aria-label="Toggle info"
      onclick={() => (viewer.infoOpen = !viewer.infoOpen)}
      class={`mr-2 shrink-0 rounded-lg p-2 ${
        viewer.infoOpen
          ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
          : 'text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800'
      }`}
    >
      <Info size={18} />
    </button>
  {/if}
</div>
