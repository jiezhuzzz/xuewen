<script lang="ts">
  import { Check, CircleAlert, Copy, Loader, Upload, X } from 'lucide-svelte';
  import { closeImport, enqueueFiles, importState } from '../lib/state.svelte';

  let dragging = $state(false);
  let input: HTMLInputElement;

  function pick(list: FileList | null) {
    if (!list) return;
    const files = Array.from(list).filter(
      (f) => /\.pdf$/i.test(f.name) || f.type === 'application/pdf',
    );
    if (files.length) void enqueueFiles(files);
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    pick(e.dataTransfer?.files ?? null);
  }

  const summary = $derived.by(() => {
    const c = { ingested: 0, duplicate: 0, failed: 0 };
    for (const i of importState.items) {
      if (i.status in c) c[i.status as keyof typeof c]++;
    }
    return c;
  });
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 p-4"
  role="dialog"
  aria-modal="true"
  aria-label="Import papers"
>
  <div class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl bg-white shadow-xl dark:bg-slate-900">
    <div class="flex items-center justify-between border-b border-slate-200 p-4 dark:border-slate-800">
      <h2 class="text-base font-semibold">Import papers</h2>
      <button
        type="button"
        onclick={closeImport}
        aria-label="Close import"
        class="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <X size={18} />
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <button
        type="button"
        onclick={() => input.click()}
        ondragover={(e) => {
          e.preventDefault();
          dragging = true;
        }}
        ondragleave={() => (dragging = false)}
        ondrop={onDrop}
        class="flex w-full flex-col items-center gap-2 rounded-xl border-2 border-dashed p-8 text-sm transition-colors {dragging
          ? 'border-indigo-400 bg-indigo-50 dark:bg-indigo-500/10'
          : 'border-slate-300 dark:border-slate-700'}"
      >
        <Upload size={24} class="pointer-events-none text-slate-400" />
        <span class="pointer-events-none text-slate-600 dark:text-slate-300">Drag PDFs here, or click to browse</span>
      </button>
      <input
        bind:this={input}
        type="file"
        accept=".pdf,application/pdf"
        multiple
        class="hidden"
        onchange={(e) => pick((e.currentTarget as HTMLInputElement).files)}
      />

      {#if importState.items.length}
        <ul class="mt-4 space-y-1">
          {#each importState.items as item, i (i)}
            <li class="flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm">
              {#if item.status === 'importing'}
                <Loader size={14} class="shrink-0 animate-spin text-indigo-500" />
              {:else if item.status === 'ingested'}
                <Check size={14} class="shrink-0 text-emerald-500" />
              {:else if item.status === 'duplicate'}
                <Copy size={14} class="shrink-0 text-slate-400" />
              {:else if item.status === 'failed'}
                <CircleAlert size={14} class="shrink-0 text-red-500" />
              {:else}
                <span class="h-3.5 w-3.5 shrink-0 rounded-full border border-slate-300 dark:border-slate-600"></span>
              {/if}
              <span class="min-w-0 flex-1 truncate text-slate-700 dark:text-slate-200">{item.name}</span>
              {#if item.status === 'ingested'}
                <span class="flex max-w-[55%] shrink-0 items-center justify-end gap-1.5 text-xs">
                  {#if item.needsReview}
                    <span class="shrink-0 rounded bg-amber-100 px-1.5 py-0.5 font-medium text-amber-700 dark:bg-amber-500/15 dark:text-amber-400">needs review</span>
                  {/if}
                  <span class="truncate text-slate-500 dark:text-slate-400" title={item.message}>{item.message}</span>
                </span>
              {:else}
                <span
                  class="max-w-[45%] shrink-0 truncate text-right text-xs text-slate-500 dark:text-slate-400"
                  title={item.message}
                >
                  {#if item.status === 'duplicate'}duplicate
                  {:else if item.status === 'failed'}{item.message}
                  {:else if item.status === 'importing'}importing…
                  {:else}queued{/if}
                </span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if importState.items.length}
      <div class="border-t border-slate-200 p-3 text-xs text-slate-500 dark:border-slate-800 dark:text-slate-400">
        {summary.ingested} ingested, {summary.duplicate} duplicate, {summary.failed} failed
      </div>
    {/if}
  </div>
</div>
