<script lang="ts">
  import { Info, Maximize2, Minimize2 } from 'lucide-svelte';
  import { chat } from '../lib/chat.svelte';
  import { DUR, dur, EASE } from '../lib/motion';
  import { appSettings, toggleInfo, toggleZen, ui, viewer } from '../lib/state.svelte';
  import { setTranslateMode, translateMode } from '../lib/translate.svelte';
  import type { PillHide } from '../lib/pillHide.svelte';

  let { pill }: { pill: PillHide } = $props();

  let showModeMenu = $state(false);
  let translateWrap = $state<HTMLDivElement | null>(null);

  // Click-outside-to-close and Escape, mirroring FilterRow's per-pill "⋯"
  // menu: the keydown handler lives on the plain wrapper div (not the
  // role="menu" popover itself), which avoids svelte-check's
  // a11y_interactive_supports_focus.
  function onWindowPointerDown(e: PointerEvent) {
    if (!showModeMenu) return;
    if (translateWrap && e.target instanceof Node && translateWrap.contains(e.target)) return;
    showModeMenu = false;
  }
  function onTranslateWrapKeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    showModeMenu = false;
  }

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
  {#if appSettings.translate.enabled}
    <!-- svelte-ignore a11y_no_static_element_interactions -- the keydown
         only handles Escape (see onTranslateWrapKeydown); every interactive
         control here is a real sibling button, never nested. -->
    <div class="relative" bind:this={translateWrap} onkeydown={onTranslateWrapKeydown}>
      <button
        type="button"
        class={translateMode.value === 'manual' ? activeBtn : btn}
        aria-label="Translate mode"
        aria-expanded={showModeMenu}
        title="Translate on selection"
        onclick={() => (showModeMenu = !showModeMenu)}
      >
        <span class="font-serif text-base leading-none">譯</span>
      </button>
      {#if showModeMenu}
        <div
          role="menu"
          class="absolute right-0 top-full z-30 mt-1 w-40 rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
        >
          <p class="px-1 pb-1 text-[10px] font-semibold uppercase tracking-wide text-stone-400">
            Translate on selection
          </p>
          <div class="flex overflow-hidden rounded-lg border border-stone-300 text-xs dark:border-stone-600">
            {#each [{ value: 'auto', label: 'Auto' }, { value: 'manual', label: 'Manual' }] as m (m.value)}
              <button
                type="button"
                aria-pressed={translateMode.value === m.value}
                onclick={() => {
                  setTranslateMode(m.value as 'auto' | 'manual');
                  showModeMenu = false;
                }}
                class={`flex-1 py-1 ${
                  translateMode.value === m.value
                    ? 'bg-amber-700/10 text-amber-700 dark:text-amber-500'
                    : 'text-stone-500'
                }`}
              >{m.label}</button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<svelte:window onpointerdown={onWindowPointerDown} />
