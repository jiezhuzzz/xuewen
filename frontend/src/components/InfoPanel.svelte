<script lang="ts">
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { detailRefresh, loadDetail } from '../lib/state.svelte';
  import CiteActions from './CiteActions.svelte';
  import PaperActions from './PaperActions.svelte';
  import PaperMeta from './PaperMeta.svelte';
  import ProjectTags from './ProjectTags.svelte';

  let { id }: { id: string } = $props();
</script>

<aside
  transition:fly={{ x: 24, duration: dur(DUR.base) }}
  class="flex h-full w-80 shrink-0 flex-col overflow-y-auto border-l border-stone-200 bg-paper p-4 dark:border-stone-800 dark:bg-night"
>
  {#key `${id}-${detailRefresh.n}`}
    {#await loadDetail(id)}
      <p class="text-sm text-stone-500 dark:text-stone-400">Loading…</p>
    {:then d}
      <PaperMeta {d} />
      <div class="mt-4"><ProjectTags {d} /></div>
      {#if d.abstract}
        <section class="mt-4">
          <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">
            Abstract
          </h3>
          <p class="text-sm leading-relaxed text-stone-600 dark:text-stone-300">{d.abstract}</p>
        </section>
      {/if}
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">Cite</h3>
        <CiteActions id={d.id} citeKey={d.cite_key} />
      </div>
      <div class="mt-6 border-t border-stone-200 pt-4 dark:border-stone-800">
        <PaperActions {d} />
      </div>
    {:catch}
      <p class="text-sm text-red-600 dark:text-red-400">Failed to load details.</p>
    {/await}
  {/key}
</aside>
