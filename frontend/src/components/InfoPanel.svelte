<script lang="ts">
  import { Star, Wand2, X } from 'lucide-svelte';
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import {
    appSettings,
    detailRefresh,
    loadDetail,
    openIdentify,
    setInfoOpen,
    toggleStar,
  } from '../lib/state.svelte';
  import CiteActions from './CiteActions.svelte';
  import DeletePaperButton from './DeletePaperButton.svelte';
  import PaperMeta from './PaperMeta.svelte';
  import PaperProjectEditor from './PaperProjectEditor.svelte';
  import PaperTagEditor from './PaperTagEditor.svelte';

  let { id }: { id: string } = $props();

  let abstractOpen = $state(!appSettings.foldAbstract);

  function fmtDate(s: string): string {
    if (!s) return '—';
    const dt = new Date(s);
    return isNaN(dt.getTime())
      ? s
      : dt.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  }

  const label = 'text-[11px] font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400';
  const divider = 'mt-4 border-t border-stone-200 pt-4 dark:border-stone-800';
</script>

<aside
  transition:fly={{ x: 24, duration: dur(DUR.base) }}
  aria-label="Paper details"
  class="absolute inset-y-3 right-3 z-20 flex w-80 max-w-[calc(100%-1.5rem)] flex-col overflow-hidden rounded-2xl border border-stone-200 bg-paper shadow-2xl dark:border-stone-800 dark:bg-soot"
>
  <div class="flex items-center justify-between border-b border-stone-200 px-4 py-2.5 dark:border-stone-800">
    <span class={label}>Details</span>
    <button
      type="button"
      aria-label="Close details"
      onclick={() => setInfoOpen(false)}
      class="rounded-lg p-1 text-stone-500 hover:bg-parchment hover:text-ink dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
    >
      <X size={16} />
    </button>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto px-4 py-4">
    {#key `${id}-${detailRefresh.n}`}
      {#await loadDetail(id)}
        <p class="text-sm text-stone-500 dark:text-stone-400">Loading…</p>
      {:then d}
        <PaperMeta {d} />

        {#if d.summary}
          <section class={divider}>
            <h3 class={label}>Summary</h3>
            <p class="mt-2 font-serif text-[13.5px] font-medium leading-relaxed text-ink dark:text-stone-100">
              {d.summary.tldr}
            </p>
            <dl class="mt-2 space-y-1.5 text-[12.5px]">
              {#each [['Problem', d.summary.problem], ['Approach', d.summary.approach], ['Results', d.summary.results], ['Limitations', d.summary.limitations]] as [k, v]}
                <div>
                  <dt class="text-[11px] font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400">{k}</dt>
                  <dd class="text-stone-700 dark:text-stone-300">{v}</dd>
                </div>
              {/each}
            </dl>
          </section>
        {/if}

        {#if d.abstract}
          <section class={divider}>
            <button type="button" onclick={() => (abstractOpen = !abstractOpen)} aria-expanded={abstractOpen} class={`flex items-center gap-1.5 ${label}`}>
              Abstract
              <svg
                class={`h-3 w-3 transition-transform ${abstractOpen ? '' : '-rotate-90'}`}
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"
                stroke-linecap="round" stroke-linejoin="round"
              ><path d="m6 9 6 6 6-6" /></svg>
            </button>
            {#if abstractOpen}
              <p class="mt-2 max-w-[42ch] font-serif text-[13.5px] leading-relaxed text-stone-700 dark:text-stone-300">
                {d.abstract}
              </p>
            {/if}
          </section>
        {/if}

        <section class={divider}>
          <h3 class={label}>Record</h3>
          <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[12.5px]">
            {#if d.cite_key}
              <dt class="text-stone-500 dark:text-stone-400">Cite key</dt>
              <dd class="font-mono text-[11.5px] text-ink dark:text-stone-200">{d.cite_key}</dd>
            {/if}
            {#if d.source}
              <dt class="text-stone-500 dark:text-stone-400">Source</dt>
              <dd class="text-ink dark:text-stone-200">{d.source}</dd>
            {/if}
            <dt class="text-stone-500 dark:text-stone-400">Added</dt>
            <dd class="text-ink dark:text-stone-200">{fmtDate(d.added_at)}</dd>
          </dl>
        </section>

        <section class={divider}>
          <h3 class={label}>Organize</h3>
          <button
            type="button"
            aria-pressed={d.starred}
            onclick={() => void toggleStar(d.id)}
            class={`mt-2 inline-flex items-center gap-1.5 rounded-lg border px-2 py-1 text-xs font-medium ${
              d.starred
                ? 'border-orange-600/50 bg-orange-600/15 text-orange-700 dark:border-orange-400/50 dark:bg-orange-400/15 dark:text-orange-400'
                : 'border-stone-200 text-stone-500 hover:border-orange-600/35 hover:text-orange-700 dark:border-stone-700 dark:text-stone-400'
            }`}
          >
            <Star size={13} fill={d.starred ? 'currentColor' : 'none'} />
            {d.starred ? 'Starred' : 'Star'}
          </button>
          <div class="mt-3">
            <PaperTagEditor {d} />
          </div>
          <div class="mt-4">
            <PaperProjectEditor {d} />
          </div>
        </section>

        <div class={`flex flex-wrap items-center gap-2 ${divider}`}>
          <CiteActions id={d.id} citeKey={d.cite_key} />
          <button
            type="button"
            onclick={() => openIdentify(d.id, { doi: d.doi, arxiv_id: d.arxiv_id })}
            class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
          >
            <Wand2 size={13} /> Identify…
          </button>
        </div>
      {:catch}
        <p class="text-sm text-red-600 dark:text-red-400">
          Failed to load details. Check that the server is running, then reopen this panel.
        </p>
      {/await}
    {/key}
  </div>

  <div class="flex flex-col items-center border-t border-stone-200 px-4 py-3 dark:border-stone-800">
    <DeletePaperButton {id} />
  </div>
</aside>
