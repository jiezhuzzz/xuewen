<script lang="ts">
  import { X } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import { addToProject, projects, removeFromProject } from '../lib/state.svelte';

  let { d }: { d: PaperDetail } = $props();

  let membershipError = $state<string | null>(null);

  async function onAddProject(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    // The "New project…" sentinel is handled by the pill-bar's own inline
    // creation affordance now (see FilterRow.svelte); this dropdown no
    // longer opens a modal for it. (Task 13 removes the sentinel entirely
    // along with the rest of this component's project_ids migration.)
    if (projectId === '__new__') {
      return;
    }
    membershipError = null;
    try {
      await addToProject(d.id, projectId);
    } catch (err) {
      membershipError = (err as Error).message;
    }
  }

  async function onRemoveProject(projectId: string) {
    membershipError = null;
    try {
      await removeFromProject(d.id, projectId);
    } catch (err) {
      membershipError = (err as Error).message;
    }
  }

  function projectName(pid: string): string {
    return projects.items.find((p) => p.id === pid)?.name ?? pid;
  }
</script>

<h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">Projects</h3>
{#if d.project_ids.length}
  <div class="flex flex-wrap gap-1.5">
    {#each d.project_ids as pid (pid)}
      <span
        class="inline-flex items-center gap-1 rounded-full bg-parchment px-2 py-0.5 text-xs text-stone-700 dark:bg-stone-800 dark:text-stone-300"
      >
        {projectName(pid)}
        <button
          type="button"
          aria-label={`Remove from ${projectName(pid)}`}
          onclick={() => void onRemoveProject(pid)}
          class="rounded-full hover:bg-stone-200 dark:hover:bg-stone-700"
        >
          <X size={12} />
        </button>
      </span>
    {/each}
  </div>
{/if}
<select
  aria-label="Add to project"
  onchange={onAddProject}
  class="mt-2 w-full rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
>
  <option value="">Add to project…</option>
  {#each projects.items.filter((p) => !d.project_ids.includes(p.id)) as p (p.id)}
    <option value={p.id}>{p.name}</option>
  {/each}
  <option value="__new__">New project…</option>
</select>
{#if membershipError}
  <p class="mt-1 text-xs text-red-600 dark:text-red-400">{membershipError}</p>
{/if}
