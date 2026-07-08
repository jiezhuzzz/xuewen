<script lang="ts">
  import { ExternalLink } from 'lucide-svelte';
  import { loadDetail } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';

  let { id }: { id: string } = $props();

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
    {#if d.abstract}
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">Abstract</h3>
        <p class="text-sm leading-relaxed text-slate-600 dark:text-slate-300">{d.abstract}</p>
      </div>
    {/if}
  {:catch}
    <p class="text-sm text-red-600 dark:text-red-400">Failed to load details.</p>
  {/await}
</aside>
