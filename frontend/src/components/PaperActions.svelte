<script lang="ts">
  import { Trash2, Wand2 } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import { openIdentify, removePaper } from '../lib/state.svelte';
  import { toast } from '../lib/toasts.svelte';

  let { d }: { d: PaperDetail } = $props();

  let confirming = $state(false);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  async function doDelete() {
    deleting = true;
    deleteError = null;
    try {
      await removePaper(d.id);
      toast('success', 'Paper deleted');
      // On success the surrounding view unmounts (tab closes / selection clears).
    } catch (e) {
      deleteError = (e as Error).message;
      deleting = false;
    }
  }
</script>

<div class="flex flex-wrap items-center gap-3">
  <button
    type="button"
    onclick={() => openIdentify(d.id, { doi: d.doi, arxiv_id: d.arxiv_id })}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-3 py-1.5 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
  >
    <Wand2 size={14} /> Identify…
  </button>
  {#if confirming}
    {#if deleting}
      <span class="text-sm text-stone-500 dark:text-stone-400">Deleting…</span>
    {:else}
      <span class="text-sm text-stone-600 dark:text-stone-300">Delete this paper?</span>
      <button
        type="button"
        onclick={doDelete}
        class="rounded-lg bg-red-600 px-3 py-1 text-xs font-medium text-white hover:bg-red-700"
      >
        Delete
      </button>
      <button
        type="button"
        onclick={() => (confirming = false)}
        class="rounded-lg px-3 py-1 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
      >
        Cancel
      </button>
    {/if}
  {:else}
    <button
      type="button"
      onclick={() => (confirming = true)}
      class="inline-flex items-center gap-1.5 rounded-lg border border-red-200 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 dark:border-red-900/50 dark:text-red-400 dark:hover:bg-red-500/10"
    >
      <Trash2 size={14} /> Delete paper
    </button>
  {/if}
</div>
{#if deleteError}
  <p class="mt-2 text-xs text-red-600 dark:text-red-400">Delete failed: {deleteError}</p>
{/if}
