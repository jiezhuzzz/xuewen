<script lang="ts">
  import { ChevronRight } from 'lucide-svelte';
  import { useBookmarkCapability } from '@embedpdf/plugin-bookmark/svelte';
  import { useScrollCapability } from '@embedpdf/plugin-scroll/svelte';
  import { toOutline, type OutlineNode } from '../lib/outline';

  let { documentId }: { documentId: string } = $props();
  const bookmarks = useBookmarkCapability();
  const scroll = useScrollCapability();

  // null = loading; [] = none (or the fetch failed — same quiet empty state).
  let nodes = $state<OutlineNode[] | null>(null);
  let requested = false;
  $effect(() => {
    const cap = bookmarks.provides;
    if (!cap || requested) return;
    requested = true;
    cap.forDocument(documentId).getBookmarks().wait(
      ({ bookmarks: list }) => (nodes = toOutline(list)),
      () => (nodes = []),
    );
  });

  // Collapsed nodes, keyed by index path ("0.2.1") — titles can repeat.
  let collapsed = $state<Set<string>>(new Set());
  function toggleNode(path: string): void {
    const next = new Set(collapsed);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    collapsed = next;
  }

  function jump(n: OutlineNode): void {
    if (n.pageIndex === null) return;
    scroll.provides?.forDocument(documentId).scrollToPage({ pageNumber: n.pageIndex + 1 });
  }
</script>

{#snippet row(n: OutlineNode, path: string)}
  <li>
    <div class="flex items-center" style:padding-left="{n.depth * 0.75}rem">
      {#if n.children.length > 0}
        <button
          type="button"
          aria-label={collapsed.has(path) ? 'Expand section' : 'Collapse section'}
          aria-expanded={!collapsed.has(path)}
          onclick={() => toggleNode(path)}
          class="shrink-0 rounded p-0.5 text-stone-400 hover:text-ink dark:text-stone-500 dark:hover:text-stone-100"
        >
          <span class={`block transition-transform ${collapsed.has(path) ? '' : 'rotate-90'}`}>
            <ChevronRight size={12} />
          </span>
        </button>
      {:else}
        <span class="w-4 shrink-0"></span>
      {/if}
      <button
        type="button"
        disabled={n.pageIndex === null}
        onclick={() => jump(n)}
        title={n.title}
        class="min-w-0 flex-1 truncate rounded px-1 py-0.5 text-left text-xs text-stone-600 hover:bg-parchment hover:text-ink disabled:cursor-default disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800"
      >
        {n.title}
      </button>
    </div>
    {#if n.children.length > 0 && !collapsed.has(path)}
      <ul>
        {#each n.children as c, i (i)}
          {@render row(c, `${path}.${i}`)}
        {/each}
      </ul>
    {/if}
  </li>
{/snippet}

<div class="min-h-0 flex-1 overflow-y-auto p-1.5">
  {#if nodes === null}
    <p class="p-2 text-xs text-stone-500 dark:text-stone-400">Loading…</p>
  {:else if nodes.length === 0}
    <p class="p-2 text-xs text-stone-500 dark:text-stone-400">No outline in this PDF.</p>
  {:else}
    <ul>
      {#each nodes as n, i (i)}
        {@render row(n, String(i))}
      {/each}
    </ul>
  {/if}
</div>
