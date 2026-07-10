<script lang="ts">
  import { Check, Loader, Plus, Trash2 } from 'lucide-svelte';
  import {
    closeProjects,
    createNewProject,
    projects,
    removeProject,
    renameProject,
  } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let newName = $state('');
  let newNote = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmingId = $state<string | null>(null);

  async function add() {
    const name = newName.trim();
    if (!name) return;
    busy = true;
    error = null;
    try {
      await createNewProject(name, newNote.trim() || null);
      newName = '';
      newNote = '';
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  // Rename on blur: skip the call when the name is unchanged; on an empty value
  // reset the field back to the current name rather than saving a blank.
  async function saveName(id: string, current: string, e: FocusEvent) {
    const input = e.currentTarget as HTMLInputElement;
    const name = input.value.trim();
    if (!name) {
      input.value = current;
      return;
    }
    if (name === current) return;
    try {
      await renameProject(id, { name });
    } catch (err) {
      error = (err as Error).message;
      input.value = current;
    }
  }

  async function saveNote(id: string, note: string) {
    try {
      await renameProject(id, { note: note.trim() });
    } catch (e) {
      error = (e as Error).message;
    }
  }

  async function remove(id: string) {
    confirmingId = null;
    try {
      await removeProject(id);
    } catch (e) {
      error = (e as Error).message;
    }
  }
</script>

<Modal title="Projects" onclose={closeProjects}>
  <form class="flex gap-2" onsubmit={(e) => { e.preventDefault(); void add(); }}>
    <input
      bind:value={newName}
      placeholder="New project name…"
      class="min-w-0 flex-1 rounded-lg border border-stone-300 px-3 py-1.5 text-sm dark:border-stone-700 dark:bg-stone-800"
    />
    <input
      bind:value={newNote}
      placeholder="Note (optional)"
      class="min-w-0 flex-1 rounded-lg border border-stone-300 px-3 py-1.5 text-sm dark:border-stone-700 dark:bg-stone-800"
    />
    <button
      type="submit"
      disabled={busy}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      {#if busy}<Loader size={14} class="animate-spin" />{:else}<Plus size={14} />{/if}
      Add
    </button>
  </form>

  {#if error}
    <p class="mt-3 text-sm text-red-600 dark:text-red-400">{error}</p>
  {/if}

  {#if projects.items.length === 0}
    <p class="mt-4 text-sm text-stone-500 dark:text-stone-400">No projects yet.</p>
  {:else}
    <ul class="mt-4 space-y-2">
      {#each projects.items as p (p.id)}
        <li class="rounded-lg border border-stone-200 p-2 dark:border-stone-700">
          <div class="flex items-center justify-between gap-2">
            <input
              value={p.name}
              aria-label={`Rename ${p.name}`}
              onblur={(e) => void saveName(p.id, p.name, e)}
              class="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 text-sm font-medium hover:border-stone-200 focus:border-amber-500 focus:outline-none dark:hover:border-stone-700"
            />
            <span class="flex shrink-0 items-center gap-2">
              <span class="text-xs text-stone-500 dark:text-stone-400">{p.paper_count}</span>
              {#if confirmingId === p.id}
                <button
                  type="button"
                  onclick={() => void remove(p.id)}
                  class="rounded-lg bg-red-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-red-700"
                >
                  Delete
                </button>
                <button
                  type="button"
                  onclick={() => (confirmingId = null)}
                  class="rounded-lg px-2 py-0.5 text-xs text-stone-500 hover:bg-parchment dark:text-stone-400 dark:hover:bg-stone-800"
                >
                  Cancel
                </button>
              {:else}
                <button
                  type="button"
                  aria-label={`Delete ${p.name}`}
                  onclick={() => (confirmingId = p.id)}
                  class="rounded p-1 text-stone-400 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-500/10"
                >
                  <Trash2 size={14} />
                </button>
              {/if}
            </span>
          </div>
          {#if confirmingId === p.id}
            <p class="mt-1 text-xs text-stone-600 dark:text-stone-300">Delete {p.name}?</p>
          {/if}
          <input
            value={p.note ?? ''}
            placeholder="Add a note…"
            onblur={(e) => void saveNote(p.id, (e.currentTarget as HTMLInputElement).value)}
            class="mt-1 w-full rounded border border-transparent bg-transparent px-1 py-0.5 text-xs text-stone-600 hover:border-stone-200 focus:border-amber-500 focus:outline-none dark:text-stone-300 dark:hover:border-stone-700"
          />
        </li>
      {/each}
    </ul>
  {/if}
  {#snippet footer()}
    <div class="text-right">
      <button
        type="button"
        onclick={closeProjects}
        class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
      >
        <Check size={14} /> Done
      </button>
    </div>
  {/snippet}
</Modal>
