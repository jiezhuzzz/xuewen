<script lang="ts">
  import { Check, Loader, Search, X } from 'lucide-svelte';
  import {
    applyIdentify,
    closeIdentify,
    dropsIdentifier,
    identifyState,
    runIdentifySearch,
  } from '../lib/state.svelte';

  // The staged identifier's value (direct never holds the candidate variant).
  const directValue = $derived(
    identifyState.direct && 'doi' in identifyState.direct
      ? identifyState.direct.doi
      : identifyState.direct && 'arxiv_id' in identifyState.direct
        ? identifyState.direct.arxiv_id
        : null,
  );
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 p-4"
  role="dialog"
  aria-modal="true"
  aria-label="Identify paper"
>
  <div class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl bg-white shadow-xl dark:bg-slate-900">
    <div class="flex items-center justify-between border-b border-slate-200 p-4 dark:border-slate-800">
      <h2 class="text-base font-semibold">Identify paper</h2>
      <button
        type="button"
        onclick={closeIdentify}
        aria-label="Close identify"
        class="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <X size={18} />
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <form
        class="flex gap-2"
        onsubmit={(e) => {
          e.preventDefault();
          void runIdentifySearch();
        }}
      >
        <input
          bind:value={identifyState.input}
          placeholder="DOI, arXiv id, or corrected title…"
          class="min-w-0 flex-1 rounded-lg border border-slate-300 px-3 py-1.5 text-sm dark:border-slate-700 dark:bg-slate-800"
        />
        <button
          type="submit"
          disabled={identifyState.busy}
          class="inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
        >
          {#if identifyState.busy}<Loader size={14} class="animate-spin" />{:else}<Search size={14} />{/if}
          Search
        </button>
      </form>

      {#if identifyState.error}
        <p class="mt-3 text-sm text-red-600 dark:text-red-400">{identifyState.error}</p>
      {/if}

      {#if identifyState.direct}
        <p class="mt-3 text-sm text-slate-600 dark:text-slate-300">
          Direct identifier detected ({directValue}) — Apply fetches the authoritative record.
        </p>
      {/if}

      {#if identifyState.candidates.length}
        <ul class="mt-4 space-y-1">
          {#each identifyState.candidates as c, i (i)}
            <li>
              <button
                type="button"
                onclick={() => (identifyState.selected = c)}
                class="w-full rounded-lg border px-3 py-2 text-left text-sm {identifyState.selected === c
                  ? 'border-indigo-400 bg-indigo-50 dark:bg-indigo-500/10'
                  : 'border-slate-200 hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800'}"
              >
                <span class="block font-medium">{c.title ?? '(untitled)'}</span>
                <span class="block truncate text-xs text-slate-500 dark:text-slate-400">
                  {c.authors.join(', ')}
                </span>
                <span class="block text-xs text-slate-500 dark:text-slate-400">
                  {c.venue ?? '?'} {c.year ?? ''}
                  <span class="ml-1 rounded bg-slate-100 px-1 py-0.5 dark:bg-slate-800">{c.source}</span>
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if identifyState.selected || identifyState.direct}
      <div class="border-t border-slate-200 p-3 dark:border-slate-800">
        {#if dropsIdentifier(identifyState)}
          <p class="mb-2 text-xs text-amber-600 dark:text-amber-400">
            Applying this match will drop an identifier the paper currently has
            (DOI/arXiv id not present in the selected record).
          </p>
        {/if}
        <button
          type="button"
          onclick={() => void applyIdentify()}
          disabled={identifyState.busy}
          class="inline-flex items-center gap-1.5 rounded-lg bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-700 disabled:opacity-50"
        >
          <Check size={14} /> Apply match
        </button>
      </div>
    {/if}
  </div>
</div>
