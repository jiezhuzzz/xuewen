<script lang="ts">
  import { MoreHorizontal, Trash2, Wand2 } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import { openIdentify, removePaper } from '../lib/state.svelte';
  import { toast } from '../lib/toasts.svelte';

  let { d }: { d: PaperDetail } = $props();

  let open = $state(false);
  let confirming = $state(false);
  let deleting = $state(false);
  let error = $state<string | null>(null);

  function close() {
    open = false;
    confirming = false;
    error = null;
  }

  async function doDelete() {
    deleting = true;
    error = null;
    try {
      await removePaper(d.id);
      toast('success', 'Paper deleted');
      // On success the surrounding panel unmounts (tab closes).
    } catch (e) {
      error = (e as Error).message;
      deleting = false;
    }
  }

  const item =
    'flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-parchment dark:hover:bg-stone-800';
</script>

<div class="relative">
  <button
    type="button"
    aria-label="More actions"
    aria-expanded={open}
    onclick={() => (open ? close() : (open = true))}
    class="inline-flex items-center justify-center rounded-lg border border-stone-200 p-1.5 text-stone-500 hover:text-ink dark:border-stone-700 dark:text-stone-400 dark:hover:text-stone-100"
  >
    <MoreHorizontal size={16} />
  </button>

  {#if open}
    <!-- click-away backdrop -->
    <div class="fixed inset-0 z-10" onclick={close} role="presentation"></div>
    <div
      role="menu"
      class="absolute right-0 z-20 mt-1 w-52 rounded-lg border border-stone-200 bg-paper p-1 shadow-xl dark:border-stone-700 dark:bg-soot"
    >
      <button
        type="button"
        role="menuitem"
        onclick={() => {
          close();
          openIdentify(d.id, { doi: d.doi, arxiv_id: d.arxiv_id });
        }}
        class={`${item} text-amber-700 dark:text-amber-500`}
      >
        <Wand2 size={14} /> Identify…
      </button>

      {#if confirming}
        {#if deleting}
          <span class="block px-2 py-1.5 text-sm text-stone-500 dark:text-stone-400">Deleting…</span>
        {:else}
          <div class="px-2 py-1.5">
            <p class="text-xs text-stone-600 dark:text-stone-300">Delete this paper?</p>
            <div class="mt-1.5 flex gap-2">
              <button
                type="button"
                onclick={doDelete}
                class="rounded bg-red-600 px-2 py-1 text-xs font-medium text-white hover:bg-red-700"
              >
                Delete
              </button>
              <button
                type="button"
                onclick={() => { confirming = false; error = null; }}
                class="rounded px-2 py-1 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
              >
                Cancel
              </button>
            </div>
          </div>
        {/if}
      {:else}
        <button
          type="button"
          role="menuitem"
          onclick={() => (confirming = true)}
          class={`${item} text-red-600 dark:text-red-400`}
        >
          <Trash2 size={14} /> Delete paper
        </button>
      {/if}

      {#if error}
        <p class="px-2 py-1 text-xs text-red-600 dark:text-red-400">Delete failed: {error}</p>
      {/if}
    </div>
  {/if}
</div>
