<script lang="ts">
  import { Check, Copy, Download, ExternalLink, Trash2, Wand2, X } from 'lucide-svelte';
  import {
    addToProject,
    bibFormat,
    copyCitation,
    detailRefresh,
    loadDetail,
    openIdentify,
    openProjects,
    projects,
    removeFromProject,
    removePaper,
  } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';

  let { id }: { id: string } = $props();

  let copied = $state(false);
  async function doCopy() {
    try {
      await copyCitation(id);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* clipboard blocked (insecure context) — the Download link still works */
    }
  }

  let confirming = $state(false);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  async function doDelete() {
    deleting = true;
    deleteError = null;
    try {
      await removePaper(id);
      // On success the tab closes and this panel unmounts.
    } catch (e) {
      deleteError = (e as Error).message;
      deleting = false;
    }
  }

  let membershipError = $state<string | null>(null);

  async function onAddProject(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    // The sentinel option opens the Projects modal instead of adding.
    if (projectId === '__new__') {
      openProjects();
      return;
    }
    membershipError = null;
    try {
      await addToProject(id, projectId);
    } catch (err) {
      membershipError = (err as Error).message;
    }
  }

  async function onRemoveProject(projectId: string) {
    membershipError = null;
    try {
      await removeFromProject(id, projectId);
    } catch (err) {
      membershipError = (err as Error).message;
    }
  }

  function projectName(pid: string): string {
    return projects.items.find((p) => p.id === pid)?.name ?? pid;
  }

  type Link = { label: string; href: string };
  function links(d: {
    doi: string | null;
    arxiv_id: string | null;
    dblp_key: string | null;
    url: string | null;
  }): Link[] {
    const out: Link[] = [];
    if (d.doi) out.push({ label: 'DOI', href: `https://doi.org/${d.doi}` });
    if (d.arxiv_id) out.push({ label: 'arXiv', href: `https://arxiv.org/abs/${d.arxiv_id}` });
    if (d.dblp_key) out.push({ label: 'DBLP', href: `https://dblp.org/rec/${d.dblp_key}.html` });
    if (d.url) out.push({ label: 'URL', href: d.url });
    return out;
  }
</script>

<aside class="flex h-full w-80 shrink-0 flex-col overflow-y-auto border-l border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
  {#key `${id}-${detailRefresh.n}`}
    {#await loadDetail(id)}
      <p class="text-sm text-slate-500 dark:text-slate-400">Loading…</p>
    {:then d}
      <h2 class="text-base font-semibold leading-snug">{d.title ?? '(untitled)'}</h2>
      <div class="mt-2"><StatusPill status={d.status} /></div>
      {#if d.authors.length}
        <p class="mt-3 text-sm text-slate-600 dark:text-slate-300">{d.authors.join(', ')}</p>
      {/if}
      <dl class="mt-3 space-y-1 text-xs text-slate-500 dark:text-slate-400">
        {#if d.venue}<div><dt class="inline font-medium">Venue:</dt> {d.venue}</div>{/if}
        {#if d.year}<div><dt class="inline font-medium">Year:</dt> {d.year}</div>{/if}
        {#if d.cite_key}<div><dt class="inline font-medium">Cite key:</dt> <code>{d.cite_key}</code></div>{/if}
        {#if d.source}<div><dt class="inline font-medium">Source:</dt> {d.source}</div>{/if}
      </dl>
      {#if links(d).length}
        <div class="mt-3 flex flex-wrap gap-2">
          {#each links(d) as l (l.label)}
            <a
              href={l.href}
              target="_blank"
              rel="noreferrer"
              class="inline-flex items-center gap-1 rounded-lg border border-slate-200 px-2 py-1 text-xs text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
            >
              {l.label}<ExternalLink size={12} />
            </a>
          {/each}
        </div>
      {/if}
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">Projects</h3>
        {#if d.project_ids.length}
          <div class="flex flex-wrap gap-1.5">
            {#each d.project_ids as pid (pid)}
              <span class="inline-flex items-center gap-1 rounded-full bg-indigo-50 px-2 py-0.5 text-xs text-indigo-700 dark:bg-indigo-500/10 dark:text-indigo-300">
                {projectName(pid)}
                <button
                  type="button"
                  aria-label={`Remove from ${projectName(pid)}`}
                  onclick={() => void onRemoveProject(pid)}
                  class="rounded-full hover:bg-indigo-100 dark:hover:bg-indigo-500/20"
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
          class="mt-2 w-full rounded-lg border border-slate-200 bg-slate-50 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-800"
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
      </div>
      {#if d.abstract}
        <div class="mt-4">
          <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">Abstract</h3>
          <p class="text-sm leading-relaxed text-slate-600 dark:text-slate-300">{d.abstract}</p>
        </div>
      {/if}
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">Cite</h3>
        <div class="flex items-center gap-2">
          <select
            bind:value={bibFormat.value}
            aria-label="Citation format"
            class="rounded-lg border border-slate-200 bg-slate-50 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-800"
          >
            <option value="bibtex">BibTeX</option>
            <option value="biblatex">BibLaTeX</option>
          </select>
          <button
            type="button"
            onclick={doCopy}
            class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2 py-1 text-xs font-medium text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
          >
            {#if copied}<Check size={12} /> Copied{:else}<Copy size={12} /> Copy{/if}
          </button>
          <a
            href={`/api/papers/${encodeURIComponent(id)}/export?format=${bibFormat.value}`}
            download={`${d.cite_key ?? id}.bib`}
            class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2 py-1 text-xs font-medium text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
          >
            <Download size={12} /> Download
          </a>
        </div>
      </div>
      <div class="mt-6 border-t border-slate-200 pt-4 dark:border-slate-800">
        <button
          type="button"
          onclick={() => openIdentify(id, { doi: d.doi, arxiv_id: d.arxiv_id })}
          class="mb-3 inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
        >
          <Wand2 size={14} /> Identify…
        </button>
        {#if confirming}
          {#if deleting}
            <span class="text-sm text-slate-500 dark:text-slate-400">Deleting…</span>
          {:else}
            <div class="flex items-center gap-2">
              <span class="text-sm text-slate-600 dark:text-slate-300">Delete this paper?</span>
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
                class="rounded-lg px-3 py-1 text-xs text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
              >
                Cancel
              </button>
            </div>
            {#if deleteError}
              <p class="mt-2 text-xs text-red-600 dark:text-red-400">Delete failed: {deleteError}</p>
            {/if}
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
    {:catch}
      <p class="text-sm text-red-600 dark:text-red-400">Failed to load details.</p>
    {/await}
  {/key}
</aside>
