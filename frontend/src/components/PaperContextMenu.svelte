<script lang="ts">
  import { tick } from 'svelte';
  import { Copy, ScanSearch, Trash2 } from 'lucide-svelte';
  import ConfirmButtons from './ConfirmButtons.svelte';
  import { clickOutside } from '../lib/clickOutside';
  import { closeContextMenu, contextMenu } from '../lib/contextMenu.svelte';
  import { menuItems, menuNavKeydown } from '../lib/menuNav';
  import { clampMenuPosition } from '../lib/popoverPosition';
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
      void tick().then(() => menuItems(menuEl)[0]?.focus());
    } else {
      prevFocus?.focus();
      prevFocus = null;
    }
  });

  function onMenuKeydown(e: KeyboardEvent) {
    if (mode === 'menu') menuNavKeydown(menuEl, e);
  }

  // Switching to the delete confirm moves focus onto its first button, so
  // Enter-ing "Delete…" flows straight into confirm-or-cancel by keyboard.
  $effect(() => {
    if (mode === 'delete') {
      void tick().then(() => menuEl?.querySelector<HTMLElement>('button')?.focus());
    }
  });

  // Re-runs when the menu resizes (mode switch).
  $effect(() => {
    if (!contextMenu.open || !menuEl) return;
    mode; // re-clamp when the delete-confirm changes the menu's height
    const p = clampMenuPosition(contextMenu.x, contextMenu.y, menuEl);
    left = p.left;
    top = p.top;
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

  function onWindowKeydown(e: KeyboardEvent) {
    if (contextMenu.open && e.key === 'Escape') closeContextMenu();
  }

  const itemClasses =
    'flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<svelte:window onkeydown={onWindowKeydown} onscroll={closeContextMenu} onblur={closeContextMenu} />

{#if contextMenu.open && contextMenu.paper}
  <!-- Dismiss on any pointerdown outside the menu. The right-click that
       opened it fires its pointerdown BEFORE the menu mounts (and with it
       the action's listener) — no immediate re-close. -->
  <div
    bind:this={menuEl}
    use:clickOutside={closeContextMenu}
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
