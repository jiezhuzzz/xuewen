<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { MoveHorizontal, RotateCcw } from 'lucide-svelte';
  import { clickOutside } from '../lib/clickOutside';
  import { menuItems, menuNavKeydown } from '../lib/menuNav';
  import { clampMenuPosition } from '../lib/popoverPosition';

  let {
    x,
    y,
    onAutoFitAll,
    onReset,
    onClose,
  }: {
    x: number;
    y: number;
    onAutoFitAll: () => void;
    onReset: () => void;
    onClose: () => void;
  } = $props();

  let menuEl = $state<HTMLDivElement | null>(null);
  let left = $state(0);
  let top = $state(0);

  // Mounted only while open (unlike the always-mounted PaperContextMenu), so
  // onMount IS the open transition: move focus in now, restore it on close.
  let prevFocus: HTMLElement | null = null;
  onMount(() => {
    prevFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    void tick().then(() => menuItems(menuEl)[0]?.focus());
    return () => prevFocus?.focus();
  });

  $effect(() => {
    if (!menuEl) return;
    const pos = clampMenuPosition(x, y, menuEl);
    left = pos.left;
    top = pos.top;
  });

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose();
  }

  const itemClasses =
    'flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<svelte:window onkeydown={onWindowKeydown} onscroll={onClose} onblur={onClose} />

<!-- Dismiss on any pointerdown outside the menu. The right-click that opened
     it fires its pointerdown BEFORE the menu mounts — no immediate re-close. -->
<div
  bind:this={menuEl}
  use:clickOutside={onClose}
  role="menu"
  aria-label="Column options"
  tabindex="-1"
  onkeydown={(e) => menuNavKeydown(menuEl, e)}
  class="fixed z-50 w-48 rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
  style={`left:${left}px;top:${top}px`}
>
  <button
    type="button"
    role="menuitem"
    class={itemClasses}
    onclick={() => {
      onAutoFitAll();
      onClose();
    }}
  >
    <MoveHorizontal size={13} /> Auto-fit all columns
  </button>
  <button
    type="button"
    role="menuitem"
    class={itemClasses}
    onclick={() => {
      onReset();
      onClose();
    }}
  >
    <RotateCcw size={13} /> Reset to default widths
  </button>
</div>
