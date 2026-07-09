<script lang="ts">
  import { Check, Loader, Plus, Trash2, X } from 'lucide-svelte';
  import {
    closeProjects,
    createNewProject,
    projects,
    removeProject,
    renameProject,
  } from '../lib/state.svelte';

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

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 p-4"
  role="dialog"
  aria-modal="true"
  aria-label="Projects"
>
  <div class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl bg-white text-slate-900 shadow-xl dark:bg-slate-900 dark:text-slate-100">
    <div class="flex items-center justify-between border-b border-slate-200 p-4 dark:border-slate-800">
      <h2 class="text-base font-semibold">Projects</h2>
      <button
        type="button"
        onclick={closeProjects}
        aria-label="Close projects"
        class="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <X size={18} />
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <form class="flex gap-2" onsubmit={(e) => { e.preventDefault(); void add(); }}>
        <input
          bind:value={newName}
          placeholder="New project name…"
          class="min-w-0 flex-1 rounded-lg border border-slate-300 px-3 py-1.5 text-sm dark:border-slate-700 dark:bg-slate-800"
        />
        <input
          bind:value={newNote}
          placeholder="Note (optional)"
          class="min-w-0 flex-1 rounded-lg border border-slate-300 px-3 py-1.5 text-sm dark:border-slate-700 dark:bg-slate-800"
        />
        <button
          type="submit"
          disabled={busy}
          class="inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
        >
          {#if busy}<Loader size={14} class="animate-spin" />{:else}<Plus size={14} />{/if}
          Add
        </button>
      </form>

      {#if error}
        <p class="mt-3 text-sm text-red-600 dark:text-red-400">{error}</p>
      {/if}

      {#if projects.items.length === 0}
        <p class="mt-4 text-sm text-slate-500 dark:text-slate-400">No projects yet.</p>
      {:else}
        <ul class="mt-4 space-y-2">
          {#each projects.items as p (p.id)}
            <li class="rounded-lg border border-slate-200 p-2 dark:border-slate-700">
              <div class="flex items-center justify-between gap-2">
                <input
                  value={p.name}
                  aria-label={`Rename ${p.name}`}
                  onblur={(e) => void saveName(p.id, p.name, e)}
                  class="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 text-sm font-medium hover:border-slate-200 focus:border-indigo-400 focus:outline-none dark:hover:border-slate-700"
                />
                <span class="flex shrink-0 items-center gap-2">
                  <span class="text-xs text-slate-500 dark:text-slate-400">{p.paper_count}</span>
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
                      class="rounded-lg px-2 py-0.5 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
                    >
                      Cancel
                    </button>
                  {:else}
                    <button
                      type="button"
                      aria-label={`Delete ${p.name}`}
                      onclick={() => (confirmingId = p.id)}
                      class="rounded p-1 text-slate-400 hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-500/10"
                    >
                      <Trash2 size={14} />
                    </button>
                  {/if}
                </span>
              </div>
              {#if confirmingId === p.id}
                <p class="mt-1 text-xs text-slate-600 dark:text-slate-300">Delete {p.name}?</p>
              {/if}
              <input
                value={p.note ?? ''}
                placeholder="Add a note…"
                onblur={(e) => void saveNote(p.id, (e.currentTarget as HTMLInputElement).value)}
                class="mt-1 w-full rounded border border-transparent bg-transparent px-1 py-0.5 text-xs text-slate-600 hover:border-slate-200 focus:border-indigo-400 focus:outline-none dark:text-slate-300 dark:hover:border-slate-700"
              />
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="border-t border-slate-200 p-3 text-right dark:border-slate-800">
      <button
        type="button"
        onclick={closeProjects}
        class="inline-flex items-center gap-1.5 rounded-lg bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-700"
      >
        <Check size={14} /> Done
      </button>
    </div>
  </div>
</div>
