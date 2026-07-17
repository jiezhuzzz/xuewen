<script lang="ts">
  import { Maximize2, Minimize2, X } from 'lucide-svelte';
  import { fly } from 'svelte/transition';
  import { chat } from '../lib/chat.svelte';
  import { DUR, dur } from '../lib/motion';
  import { closeDock, dock, openDock, toggleZen, ui } from '../lib/state.svelte';
  import DockAsk from './DockAsk.svelte';
  import DockDetails from './DockDetails.svelte';

  let { id }: { id: string } = $props();

  // Ask is only offered when chat is configured; a remembered 'ask' tab
  // degrades to Details rather than rendering a dead tab.
  const tab = $derived(dock.tab === 'ask' && !chat.available ? 'details' : dock.tab);

  const tabBase = 'rounded-lg px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[.07em]';
  const tabOff = 'text-stone-500 hover:bg-parchment hover:text-ink dark:text-stone-400 dark:hover:bg-stone-800';
  const tabOn = 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
  const iconBtn = 'rounded-lg p-1.5 text-stone-500 hover:bg-parchment hover:text-ink dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100';

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // The dock owns this Esc — it must not also exit zen.
      e.stopPropagation();
      closeDock();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the aside is
     not an interaction target; it delegates Esc bubbling up from focused
     controls so the dock can close itself (same rationale as the old
     ChatPanel). -->
<aside
  transition:fly={{ x: 24, duration: dur(DUR.base) }}
  aria-label="Paper panel"
  onkeydown={onKeydown}
  class="absolute inset-y-3 right-3 z-40 flex w-96 max-w-[calc(100%-1.5rem)] flex-col overflow-hidden rounded-2xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
>
  <div class="flex shrink-0 items-center justify-between gap-2 border-b border-stone-200 px-2.5 py-2 dark:border-stone-800">
    <div role="tablist" aria-label="Panel tabs" class="flex items-center gap-1">
      <button
        type="button"
        role="tab"
        aria-selected={tab === 'details'}
        class={`${tabBase} ${tab === 'details' ? tabOn : tabOff}`}
        onclick={() => openDock('details')}
      >Details</button>
      {#if chat.available}
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'ask'}
          class={`${tabBase} ${tab === 'ask' ? tabOn : tabOff}`}
          onclick={() => openDock('ask')}
        >Ask 問</button>
      {/if}
    </div>
    <div class="flex items-center gap-0.5">
      <button
        type="button"
        class={iconBtn}
        aria-label="Zen mode"
        title="Zen — z"
        onclick={toggleZen}
      >
        {#if ui.zen}<Minimize2 size={15} />{:else}<Maximize2 size={15} />{/if}
      </button>
      <button
        type="button"
        class={iconBtn}
        aria-label="Close panel"
        title="Close — Esc"
        onclick={closeDock}
      >
        <X size={15} />
      </button>
    </div>
  </div>

  {#if tab === 'details'}
    {#key id}
      <DockDetails {id} />
    {/key}
  {:else}
    <DockAsk />
  {/if}
</aside>
