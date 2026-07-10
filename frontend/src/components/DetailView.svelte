<script lang="ts">
  import { BookOpen } from 'lucide-svelte';
  import { fly } from 'svelte/transition';
  import { DUR, dur } from '../lib/motion';
  import { detailRefresh, loadDetail, openTab, selection } from '../lib/state.svelte';
  import CiteActions from './CiteActions.svelte';
  import PaperActions from './PaperActions.svelte';
  import PaperMeta from './PaperMeta.svelte';
  import ProjectTags from './ProjectTags.svelte';
  import Welcome from './Welcome.svelte';
</script>

{#if selection.id === null}
  <Welcome />
{:else}
  {#key `${selection.id}-${detailRefresh.n}`}
    <div class="h-full min-w-0 flex-1 overflow-y-auto">
      <article class="mx-auto max-w-3xl px-8 py-10">
        {#await loadDetail(selection.id)}
          <p class="text-sm text-stone-500 dark:text-stone-400">Loading…</p>
        {:then d}
          <header in:fly={{ y: 8, duration: dur(DUR.base) }}>
            <PaperMeta {d} hero />
          </header>
          <div
            in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(60) }}
            class="mt-6 flex flex-wrap items-center gap-3 border-y border-stone-200 py-3 dark:border-stone-800"
          >
            <button
              type="button"
              onclick={() => openTab(d)}
              class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
            >
              <BookOpen size={15} /> Open PDF
            </button>
            <CiteActions id={d.id} citeKey={d.cite_key} />
          </div>
          {#if d.abstract}
            <section in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(120) }} class="mt-6">
              <h3 class="text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">
                Abstract
              </h3>
              <p class="mt-2 max-w-[65ch] font-serif text-[15px] leading-relaxed text-stone-700 dark:text-stone-300">
                {d.abstract}
              </p>
            </section>
          {/if}
          <section in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(180) }} class="mt-6 max-w-sm">
            <ProjectTags {d} />
          </section>
          <footer
            in:fly={{ y: 8, duration: dur(DUR.base), delay: dur(240) }}
            class="mt-10 border-t border-stone-200 pt-4 dark:border-stone-800"
          >
            <PaperActions {d} />
          </footer>
        {:catch}
          <p class="text-sm text-red-600 dark:text-red-400">
            Failed to load details. Check that the server is running, then select the paper again.
          </p>
        {/await}
      </article>
    </div>
  {/key}
{/if}
