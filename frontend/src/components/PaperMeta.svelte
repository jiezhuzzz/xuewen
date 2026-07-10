<script lang="ts">
  import { ExternalLink } from 'lucide-svelte';
  import type { PaperDetail } from '../lib/types';
  import StatusPill from './StatusPill.svelte';

  let { d, hero = false }: { d: PaperDetail; hero?: boolean } = $props();

  type Link = { label: string; href: string };
  const links = $derived.by(() => {
    const out: Link[] = [];
    if (d.doi) out.push({ label: 'DOI', href: `https://doi.org/${d.doi}` });
    if (d.arxiv_id) out.push({ label: 'arXiv', href: `https://arxiv.org/abs/${d.arxiv_id}` });
    if (d.dblp_key) out.push({ label: 'DBLP', href: `https://dblp.org/rec/${d.dblp_key}.html` });
    if (d.url) out.push({ label: 'URL', href: d.url });
    return out;
  });
</script>

{#if d.venue || d.year}
  <p
    class={`font-medium uppercase tracking-widest text-stone-500 dark:text-stone-400 ${hero ? 'text-xs' : 'text-[10px]'}`}
  >
    {d.venue ?? ''}{d.venue && d.year ? ' · ' : ''}{d.year ?? ''}
  </p>
{/if}
<h2
  class={`font-serif font-semibold text-ink dark:text-stone-100 ${
    hero ? 'mt-2 text-3xl leading-tight text-balance' : 'mt-1 text-base leading-snug'
  }`}
>
  {d.title ?? '(untitled)'}
</h2>
{#if d.authors.length}
  <p class="mt-3 text-sm text-stone-600 dark:text-stone-300">{d.authors.join(', ')}</p>
{/if}
<div class="mt-3 flex flex-wrap items-center gap-1.5">
  <StatusPill status={d.status} />
  {#each links as l (l.label)}
    <a
      href={l.href}
      target="_blank"
      rel="noreferrer"
      class="inline-flex items-center gap-1 rounded-full border border-stone-200 px-2 py-0.5 font-mono text-[11px] text-stone-600 hover:border-amber-700 hover:text-amber-700 dark:border-stone-700 dark:text-stone-300 dark:hover:border-amber-500 dark:hover:text-amber-400"
    >
      {l.label}<ExternalLink size={10} />
    </a>
  {/each}
</div>
{#if d.cite_key || d.source}
  <dl class="mt-3 space-y-0.5 text-xs text-stone-500 dark:text-stone-400">
    {#if d.cite_key}
      <div><dt class="inline font-medium">Cite key</dt> <dd class="inline font-mono">{d.cite_key}</dd></div>
    {/if}
    {#if d.source}
      <div><dt class="inline font-medium">Source</dt> <dd class="inline">{d.source}</dd></div>
    {/if}
  </dl>
{/if}
