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

  // A remembered 'ask' tab with chat unavailable would leave dock.tab
  // pointing at a tab that isn't rendered — write the degrade back so the
  // i/c shortcuts and the thread-follow effect see the truth.
  $effect(() => {
    if (dock.tab === 'ask' && !chat.available) openDock('details');
  });

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
  <!-- Close/zen sit on the LEFT, tabs on the RIGHT: the quick-action rail
       (禪詳問, top-right of the reader) hides when the dock opens, and a
       follow-up click at 詳/問's old position must land on the matching
       Details/Ask tab — not on close, which used to sit there and instantly
       dismissed the dock (the classic misclick this ordering fixes). -->
  <div class="flex shrink-0 items-center justify-between gap-2 border-b border-stone-200 px-2.5 py-2 dark:border-stone-800">
    <div class="flex items-center gap-0.5">
      <button
        type="button"
        class={iconBtn}
        aria-label="Close panel"
        title="Close — Esc"
        onclick={closeDock}
      >
        <X size={15} />
      </button>
      <button
        type="button"
        class={iconBtn}
        aria-label="Zen mode"
        title="Zen — z"
        onclick={toggleZen}
      >
        {#if ui.zen}<Minimize2 size={15} />{:else}<Maximize2 size={15} />{/if}
      </button>
    </div>
    <div role="tablist" aria-label="Panel tabs" class="flex items-center gap-1">
      <button
        type="button"
        role="tab"
        id="dock-tab-details"
        aria-selected={tab === 'details'}
        aria-controls="dock-panel"
        class={`${tabBase} ${tab === 'details' ? tabOn : tabOff}`}
        onclick={() => openDock('details')}
      >Details</button>
      {#if chat.available}
        <button
          type="button"
          role="tab"
          id="dock-tab-ask"
          aria-selected={tab === 'ask'}
          aria-controls="dock-panel"
          class={`${tabBase} ${tab === 'ask' ? tabOn : tabOff}`}
          onclick={() => openDock('ask')}
        >Ask 問</button>
      {/if}
    </div>
  </div>

  <div
    role="tabpanel"
    id="dock-panel"
    aria-labelledby={tab === 'details' ? 'dock-tab-details' : 'dock-tab-ask'}
    class="flex min-h-0 flex-1 flex-col"
  >
    {#if tab === 'details'}
      {#key id}
        <DockDetails {id} />
      {/key}
    {:else}
      <DockAsk />
    {/if}
  </div>
</aside>
