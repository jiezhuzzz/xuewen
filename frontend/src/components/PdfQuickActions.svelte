<script lang="ts">
  import { chat } from '../lib/chat.svelte';
  import { DUR, dur, EASE } from '../lib/motion';
  import { dock, toggleDock, toggleZen, ui } from '../lib/state.svelte';
  import type { PillHide } from '../lib/pillHide.svelte';

  let { pill }: { pill: PillHide } = $props();

  // Three seals 禪 詳 問 — the reader's triggers speak the app's own
  // language (學問, 譯). Tooltips carry the English + shortcut.
  const btn =
    'rounded-lg px-1.5 py-1 font-serif text-base leading-none text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
  const activeBtn =
    'rounded-lg px-1.5 py-1 font-serif text-base leading-none bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
  const askBtn =
    'rounded-lg px-1.5 py-1 font-serif text-base leading-none text-amber-700 hover:bg-parchment dark:text-amber-500 dark:hover:bg-stone-800';

  // The dock header carries zen + close while open, so the pill yields.
  const hidden = $derived(dock.open || !pill.visible);
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
    hidden ? 'pointer-events-none opacity-0' : 'opacity-100'
  }`}
>
  <button
    type="button"
    class={ui.zen ? activeBtn : btn}
    aria-label={ui.zen ? 'Exit zen mode' : 'Zen mode'}
    title="Zen — z"
    onclick={toggleZen}
  >禪</button>
  <button
    type="button"
    class={btn}
    aria-label="Details"
    title="Details — i"
    onclick={() => toggleDock('details')}
  >詳</button>
  {#if chat.available}
    <button
      type="button"
      class={askBtn}
      aria-label="Ask about this paper"
      title="Ask — c"
      onclick={() => toggleDock('ask')}
    >問</button>
  {/if}
</div>
