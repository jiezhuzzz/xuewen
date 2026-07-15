<script lang="ts">
  import { Info, Maximize2, Minimize2 } from 'lucide-svelte';
  import { chat } from '../lib/chat.svelte';
  import { DUR, dur, EASE } from '../lib/motion';
  import { toggleInfo, toggleZen, ui, viewer } from '../lib/state.svelte';
  import type { PillHide } from '../lib/pillHide.svelte';

  let { pill }: { pill: PillHide } = $props();

  const btn =
    'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
  const activeBtn = 'rounded-lg p-1.5 bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -- every control inside
     the pill is individually tabbable via normal document tab order; the
     toolbar container itself is not a tab stop (same rationale as
     PdfToolbar's pill). -->
<div
  role="toolbar"
  aria-label="Reader quick actions"
  onpointerenter={() => pill.pillEnter()}
  onpointerleave={() => pill.pillLeave()}
  onfocusin={() => pill.focusIn()}
  onfocusout={() => pill.focusOut()}
  style:transition="opacity {dur(DUR.base)}ms {EASE}"
  class={`absolute right-3 top-3 z-20 flex items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90 ${
    pill.visible ? 'opacity-100' : 'pointer-events-none opacity-0'
  }`}
>
  <button
    type="button"
    class={btn}
    aria-label={ui.zen ? 'Exit zen mode' : 'Zen mode'}
    title="Zen mode (z)"
    onclick={toggleZen}
  >
    {#if ui.zen}<Minimize2 size={16} />{:else}<Maximize2 size={16} />{/if}
  </button>
  <button
    type="button"
    class={viewer.infoOpen ? activeBtn : btn}
    aria-label="Toggle info"
    aria-expanded={viewer.infoOpen}
    title="Info (i)"
    onclick={toggleInfo}
  >
    <Info size={16} />
  </button>
  {#if chat.available}
    <!-- 問 ("to ask") — the assistant's launcher, moved here from the old
         bottom-right chat launcher button; amber because it is an action. -->
    <button
      type="button"
      class="rounded-lg px-1.5 py-1 font-serif text-base leading-none text-amber-700 hover:bg-parchment dark:text-amber-500 dark:hover:bg-stone-800"
      aria-label="Chat about this paper"
      title="Chat about this paper (c)"
      onclick={() => (chat.open = true)}
    >問</button>
  {/if}
</div>
