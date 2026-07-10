<script lang="ts">
  import { Search, SlidersHorizontal } from 'lucide-svelte';
  import { scale } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import {
    filters,
    searchMeta,
    searchOpts,
    semanticBlocked,
    setSearch,
    toggleSearchEngine,
    toggleSearchField,
  } from '../lib/state.svelte';

  let optionsOpen = $state(false);
  let root = $state<HTMLElement | null>(null);
  const FIELDS = [
    ['title', 'Title'],
    ['authors', 'Authors'],
    ['abstract', 'Abstract'],
    ['body', 'Body'],
  ] as const;
  const activeCount = $derived(
    FIELDS.filter(([k]) => searchOpts[k]).length +
      Number(searchOpts.keyword) +
      Number(searchOpts.semantic && !semanticBlocked()),
  );
  // Options the user can actually turn on right now — a server-blocked
  // semantic engine doesn't count, or the "narrowed" hint would never clear.
  const availableCount = $derived(FIELDS.length + 1 + (semanticBlocked() ? 0 : 1));

  function onPointerdownOutside(e: PointerEvent) {
    if (optionsOpen && root && !root.contains(e.target as Node)) optionsOpen = false;
  }
  function onRootKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && optionsOpen) {
      // The popover owns this Esc — it must not also reach the global
      // shortcut handler (which would exit zen or close the palette).
      e.stopPropagation();
      optionsOpen = false;
    }
  }
</script>

<svelte:window onpointerdown={onPointerdownOutside} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the div is
     not an interaction target; it only delegates Esc bubbling up from the
     focused input/chips so the popover can close without the global
     shortcut handler also firing. -->
<div class="relative" role="search" bind:this={root} onkeydown={onRootKeydown}>
  <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-stone-400" />
  <input
    data-search-input
    type="search"
    aria-label="Search papers"
    placeholder="Search library…"
    value={filters.q}
    oninput={(e) => setSearch((e.currentTarget as HTMLInputElement).value)}
    class="w-full rounded-lg border border-stone-200 bg-paper py-2 pl-9 pr-9 text-sm outline-none focus:border-amber-700 focus:ring-2 focus:ring-amber-700/15 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
  />
  <button
    type="button"
    aria-label="Search options"
    aria-expanded={optionsOpen}
    onclick={() => (optionsOpen = !optionsOpen)}
    class="absolute right-2 top-1/2 -translate-y-1/2 rounded p-1 text-stone-400 hover:bg-parchment hover:text-stone-600 dark:hover:bg-stone-700 dark:hover:text-stone-300"
  >
    <SlidersHorizontal size={14} />
  </button>

  {#if optionsOpen}
    <div
      transition:scale={{ start: 0.96, duration: dur(DUR.fast) }}
      class="absolute left-0 right-0 top-full z-20 mt-1 space-y-2 rounded-lg border border-stone-200 bg-paper p-2 shadow-lg dark:border-stone-700 dark:bg-soot"
    >
      <p class="text-[10px] font-semibold uppercase tracking-wide text-stone-400">Search in</p>
      <div class="flex flex-wrap gap-1 text-[11px]">
        {#each FIELDS as [key, label] (key)}
          <button
            type="button"
            aria-pressed={searchOpts[key]}
            onclick={() => toggleSearchField(key)}
            class={`rounded-full border px-2 py-0.5 ${
              searchOpts[key]
                ? 'border-amber-700/40 bg-amber-700/10 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
                : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
            }`}
          >
            {label}
          </button>
        {/each}
      </div>
      <p class="text-[10px] font-semibold uppercase tracking-wide text-stone-400">Engines</p>
      <div class="flex flex-wrap gap-1 text-[11px]">
        <button
          type="button"
          aria-pressed={searchOpts.keyword}
          onclick={() => toggleSearchEngine('keyword')}
          class={`rounded-full border px-2 py-0.5 ${
            searchOpts.keyword
              ? 'border-lime-600/40 bg-lime-600/10 text-lime-800 dark:border-lime-500/40 dark:bg-lime-500/10 dark:text-lime-300'
              : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
          }`}
        >
          Keyword
        </button>
        <button
          type="button"
          aria-pressed={searchOpts.semantic && !semanticBlocked()}
          disabled={semanticBlocked()}
          title={searchMeta.semantic.reason ?? undefined}
          onclick={() => toggleSearchEngine('semantic')}
          class={`rounded-full border px-2 py-0.5 disabled:cursor-not-allowed disabled:opacity-40 ${
            searchOpts.semantic && !semanticBlocked()
              ? 'border-lime-600/40 bg-lime-600/10 text-lime-800 dark:border-lime-500/40 dark:bg-lime-500/10 dark:text-lime-300'
              : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
          }`}
        >
          Semantic
        </button>
      </div>
      {#if searchMeta.pending > 0}
        <p class="text-[11px] text-stone-400 dark:text-stone-500">
          indexing {searchMeta.pending} paper{searchMeta.pending === 1 ? '' : 's'}…
        </p>
      {/if}
    </div>
  {/if}
</div>
{#if activeCount < availableCount && !optionsOpen}
  <p class="mt-1 text-[10px] text-stone-400">
    Search options narrowed — review them with the sliders button.
  </p>
{/if}
