<script lang="ts">
  import { Trash2 } from 'lucide-svelte';
  import { removePaper } from '../lib/state.svelte';
  import { toast } from '../lib/toasts.svelte';

  let { id }: { id: string } = $props();

  let confirming = $state(false);
  let deleting = $state(false);
  let error = $state<string | null>(null);

  async function doDelete() {
    deleting = true;
    error = null;
    try {
      await removePaper(id);
      toast('success', 'Paper deleted');
      // On success the surrounding panel unmounts (its tab closes).
    } catch (e) {
      error = (e as Error).message;
      deleting = false;
    }
  }
</script>

{#if confirming}
  {#if deleting}
    <span class="block text-sm text-stone-500 dark:text-stone-400">Deleting…</span>
  {:else}
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
  {/if}
{:else}
  <button
    type="button"
    onclick={() => (confirming = true)}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-red-600 hover:bg-red-600/10 dark:border-stone-700 dark:text-red-400"
  >
    <Trash2 size={13} /> Delete paper
  </button>
{/if}
{#if error}
  <p class="mt-1 text-xs text-red-600 dark:text-red-400">Delete failed: {error}</p>
{/if}
