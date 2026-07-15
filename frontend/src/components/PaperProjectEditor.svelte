<script lang="ts">
  import { Bookmark, X } from 'lucide-svelte';
  import { addToProject, createNewProject, projects, removeFromProject } from '../lib/state.svelte';
  import type { PaperDetail } from '../lib/types';

  let { d }: { d: PaperDetail } = $props();

  let error = $state<string | null>(null);
  let adding = $state(false);
  let newName = $state('');
  let newInput = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (adding) newInput?.focus();
  });

  const available = $derived(projects.items.filter((p) => !d.projects.some((dp) => dp.id === p.id)));

  async function onAdd(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    error = null;
    try {
      await addToProject(d.id, projectId);
    } catch (err) {
      error = (err as Error).message;
    }
  }

  async function onRemove(projectId: string) {
    error = null;
    try {
      await removeFromProject(d.id, projectId);
    } catch (err) {
      error = (err as Error).message;
    }
  }

  function startNew() {
    newName = '';
    adding = true;
  }
  function cancelNew() {
    adding = false;
    newName = '';
  }
  async function submitNew() {
    const name = newName.trim();
    if (!name) {
      cancelNew();
      return;
    }
    cancelNew();
    error = null;
    try {
      const p = await createNewProject(name);
      await addToProject(d.id, p.id);
    } catch (err) {
      error = (err as Error).message;
    }
  }
  function onNewKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void submitNew();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelNew();
    }
  }
</script>

<h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">Projects</h3>
{#if d.projects.length}
  <div class="flex flex-wrap gap-1.5">
    {#each d.projects as project (project.id)}
      <span
        class="inline-flex items-center gap-1 rounded border border-indigo-600/30 bg-indigo-600/10 px-1.5 py-0.5 text-xs font-semibold text-indigo-800 dark:border-indigo-400/30 dark:bg-indigo-400/10 dark:text-indigo-300"
      >
        <Bookmark size={10} />
        {project.name}
        <button
          type="button"
          aria-label={`Remove from ${project.name}`}
          onclick={() => void onRemove(project.id)}
          class="rounded-full hover:bg-indigo-600/20 dark:hover:bg-indigo-400/20"
        >
          <X size={11} />
        </button>
      </span>
    {/each}
  </div>
{/if}
<div class="mt-2 flex flex-wrap items-center gap-2">
  {#if available.length}
    <select
      aria-label="Add to project"
      onchange={onAdd}
      class="min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
    >
      <option value="">Add to project…</option>
      {#each available as p (p.id)}
        <option value={p.id}>{p.name}</option>
      {/each}
    </select>
  {/if}
  {#if adding}
    <input
      bind:this={newInput}
      bind:value={newName}
      type="text"
      aria-label="New project name"
      placeholder="Project name"
      onkeydown={onNewKeydown}
      onblur={() => (newName.trim() ? void submitNew() : cancelNew())}
      class="w-28 rounded-lg border border-dashed border-indigo-600/40 bg-paper px-2 py-1 text-xs outline-none focus:border-indigo-600 dark:border-indigo-400/40 dark:bg-stone-800"
    />
  {:else}
    <button
      type="button"
      onclick={startNew}
      class="inline-flex items-center rounded-lg border border-dashed border-stone-300 px-2 py-1 text-xs text-stone-400 hover:border-stone-400 hover:text-stone-600 dark:border-stone-600 dark:text-stone-500 dark:hover:border-stone-500 dark:hover:text-stone-300"
    >
      + New project
    </button>
  {/if}
</div>
{#if error}
  <p class="mt-1 text-xs text-red-600 dark:text-red-400">{error}</p>
{/if}
