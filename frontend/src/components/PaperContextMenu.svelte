<script lang="ts">
  import { tick } from 'svelte';
  import { Copy, ScanSearch, Trash2 } from 'lucide-svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';
  import { closeContextMenu, contextMenu } from '../lib/contextMenu.svelte';
  import { copyCitation, openIdentify, removePaper } from '../lib/state.svelte';
  import { toast } from '../lib/toasts.svelte';

  // Two-step delete lives inside the menu (mirrors DeletePaperButton /
  // FilterRow's pill menu) so a right-click delete still needs a confirm.
  let mode = $state<'menu' | 'delete'>('menu');
  let busy = $state(false);
  let menuEl = $state<HTMLDivElement | null>(null);
  let left = $state(0);
  let top = $state(0);

  // Every fresh open starts on the action list, never mid-delete-confirm.
  // Focus moves into the menu on open (WAI menu pattern) and back to
  // whatever had it when the menu closes.
  let prevFocus: HTMLElement | null = null;
  $effect(() => {
    if (contextMenu.open) {
      contextMenu.paper; // re-run when the target paper changes
      mode = 'menu';
      busy = false;
      prevFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      void tick().then(() => menuItems()[0]?.focus());
    } else {
      prevFocus?.focus();
      prevFocus = null;
    }
  });

  // Roving focus: ArrowUp/Down cycle with wrap-around, Home/End jump.
  function menuItems(): HTMLElement[] {
    return menuEl ? Array.from(menuEl.querySelectorAll<HTMLElement>('[role="menuitem"]')) : [];
  }
  function onMenuKeydown(e: KeyboardEvent) {
    if (mode !== 'menu') return;
    const list = menuItems();
    if (list.length === 0) return;
    const idx = list.indexOf(document.activeElement as HTMLElement);
    const wrap = (n: number) => (n + list.length) % list.length;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      list[wrap(idx + 1)].focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      list[idx === -1 ? list.length - 1 : wrap(idx - 1)].focus();
    } else if (e.key === 'Home') {
      e.preventDefault();
      list[0].focus();
    } else if (e.key === 'End') {
      e.preventDefault();
      list[list.length - 1].focus();
    }
  }

  // Switching to the delete confirm moves focus onto its first button, so
  // Enter-ing "Delete…" flows straight into confirm-or-cancel by keyboard.
  $effect(() => {
    if (mode === 'delete') {
      void tick().then(() => menuEl?.querySelector<HTMLElement>('button')?.focus());
    }
  });

  // Clamp to the viewport so a right-click near the bottom/right edge doesn't
  // render the menu off-screen. Re-runs when the menu resizes (mode switch).
  $effect(() => {
    if (!contextMenu.open || !menuEl) return;
    mode; // re-clamp when the delete-confirm changes the menu's height
    const { offsetWidth: w, offsetHeight: h } = menuEl;
    left = Math.min(contextMenu.x, window.innerWidth - w - 8);
    top = Math.min(contextMenu.y, window.innerHeight - h - 8);
  });

  async function doCopy() {
    const paper = contextMenu.paper;
    closeContextMenu();
    if (!paper) return;
    try {
      await copyCitation(paper.id, 'bibtex');
      toast('success', 'BibTeX copied');
    } catch {
      toast('error', "Couldn't copy BibTeX");
    }
  }

  function doIdentify() {
    const paper = contextMenu.paper;
    closeContextMenu();
    if (!paper) return;
    openIdentify(paper.id, { doi: paper.doi, arxiv_id: paper.arxiv_id });
  }

  async function doDelete() {
    const paper = contextMenu.paper;
    if (!paper) return;
    busy = true;
    try {
      await removePaper(paper.id); // shows the Deleted/Undo toast itself
      closeContextMenu();
    } catch (e) {
      toast('error', `Delete failed: ${(e as Error).message}`);
      busy = false;
    }
  }

  // Any pointerdown outside the menu dismisses it. The right-click that opened
  // the menu fires its pointerdown BEFORE openContextMenu flips `open`, so it's
  // already filtered by the `!contextMenu.open` guard — no immediate re-close.
  function onWindowPointerDown(e: PointerEvent) {
    if (!contextMenu.open) return;
    if (e.target instanceof Node && menuEl?.contains(e.target)) return;
    closeContextMenu();
  }
  function onWindowKeydown(e: KeyboardEvent) {
    if (contextMenu.open && e.key === 'Escape') closeContextMenu();
  }

  const itemClasses =
    'flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<svelte:window
  onpointerdown={onWindowPointerDown}
  onkeydown={onWindowKeydown}
  onscroll={closeContextMenu}
  onblur={closeContextMenu}
/>

{#if contextMenu.open && contextMenu.paper}
  <div
    bind:this={menuEl}
    role="menu"
    aria-label="Paper actions"
    tabindex="-1"
    onkeydown={onMenuKeydown}
    class="fixed z-50 w-44 rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
    style={`left:${left}px;top:${top}px`}
  >
    {#if mode === 'menu'}
      <button type="button" role="menuitem" onclick={() => void doCopy()} class={itemClasses}>
        <Copy size={13} /> Copy BibTeX
      </button>
      <button type="button" role="menuitem" onclick={doIdentify} class={itemClasses}>
        <ScanSearch size={13} /> Identify…
      </button>
      <div class="my-1 border-t border-stone-200 dark:border-stone-800"></div>
      <button
        type="button"
        role="menuitem"
        onclick={() => (mode = 'delete')}
        class="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-red-600 hover:bg-red-600/10 dark:text-red-400"
      >
        <Trash2 size={13} /> Delete…
      </button>
    {:else if busy}
      <span class="block px-2 py-1.5 text-xs text-stone-500 dark:text-stone-400">Deleting…</span>
    {:else}
      <p class="px-1 py-0.5 text-xs text-stone-600 dark:text-stone-300">Delete this paper?</p>
      <div class="mt-1 flex justify-end gap-1">
        <ConfirmButtons
          confirmLabel="Delete"
          onConfirm={() => void doDelete()}
          onCancel={() => (mode = 'menu')}
        />
      </div>
    {/if}
  </div>
{/if}
