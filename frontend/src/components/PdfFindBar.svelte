<script lang="ts">
  import { ChevronDown, ChevronUp, X } from 'lucide-svelte';
  import { useSearch } from '@embedpdf/plugin-search/svelte';
  import { setFind } from '../lib/readerState.svelte';

  let { documentId }: { documentId: string } = $props();
  const search = useSearch(() => documentId);

  let query = $state('');
  let inputEl: HTMLInputElement | undefined = $state();
  let debounce: ReturnType<typeof setTimeout> | undefined;

  // Focus on mount; on unmount stop the session so highlights clear.
  $effect(() => {
    inputEl?.focus();
    return () => {
      clearTimeout(debounce);
      search.provides?.stopSearch();
    };
  });

  function run(): void {
    const scope = search.provides;
    if (!scope) return;
    const q = query.trim();
    if (q) scope.searchAllPages(q);
    else scope.stopSearch();
  }
  function onInput(): void {
    clearTimeout(debounce);
    debounce = setTimeout(run, 250);
  }

  const count = $derived(search.state.results.length);
  function next(): void {
    if (count) search.provides?.nextResult();
  }
  function prev(): void {
    if (count) search.provides?.previousResult();
  }
  function close(): void {
    setFind(documentId, false);
  }
  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      // Owns its Esc — the global cascade (shortcuts.ts) runs before the
      // editable-target check and would close info/zen instead.
      e.stopPropagation();
      close();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey) prev();
      else next();
    }
  }

  const btn =
    'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-40 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<div
  class="absolute left-1/2 top-14 z-20 flex -translate-x-1/2 items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90"
>
  <input
    bind:this={inputEl}
    bind:value={query}
    data-find-input={documentId}
    placeholder="Find in document"
    aria-label="Find in document"
    class="w-48 bg-transparent px-1 text-sm text-ink placeholder:text-stone-400 focus:outline-none dark:text-stone-100 dark:placeholder:text-stone-500"
    oninput={onInput}
    onkeydown={onKeydown}
  />
  <span class="min-w-12 text-center text-xs tabular-nums text-stone-500 dark:text-stone-400">
    {query.trim() ? `${count ? search.state.activeResultIndex + 1 : 0} / ${count}` : ''}
  </span>
  <button type="button" class={btn} aria-label="Previous match" disabled={!count} onclick={prev}>
    <ChevronUp size={16} />
  </button>
  <button type="button" class={btn} aria-label="Next match" disabled={!count} onclick={next}>
    <ChevronDown size={16} />
  </button>
  <button type="button" class={btn} aria-label="Close find" onclick={close}>
    <X size={16} />
  </button>
</div>
