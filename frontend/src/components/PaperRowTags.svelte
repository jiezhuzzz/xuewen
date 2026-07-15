<script lang="ts">
  import { Bookmark } from 'lucide-svelte';
  import { filters } from '../lib/state.svelte';
  import { isPrefixMatch } from '../lib/tagTree';
  import type { PaperSummary } from '../lib/types';

  let { paper }: { paper: PaperSummary } = $props();

  // Project badges never count toward this — only topical tag chips do.
  const CAP = 3;
  let expanded = $state(false);

  function isHit(tagName: string): boolean {
    return filters.tag != null && isPrefixMatch(tagName, filters.tag);
  }

  // A tag chip is visible when expanded, within the first CAP, or (even
  // beyond the cap) it matches the active tag filter — mirrors the approved
  // mock's layoutChips(): `show = expanded || i < CAP || isHit`.
  const visibleTags = $derived(
    expanded ? paper.tags : paper.tags.filter((t, i) => i < CAP || isHit(t.name)),
  );
  const hiddenCount = $derived(paper.tags.length - visibleTags.length);

  const badgeClasses =
    'inline-flex items-center gap-1 rounded border border-indigo-600/30 bg-indigo-600/10 px-1.5 py-0.5 text-[10px] font-semibold text-indigo-800 dark:border-indigo-400/30 dark:bg-indigo-400/10 dark:text-indigo-300';

  function chipClasses(hit: boolean): string {
    return hit
      ? 'rounded border border-amber-700/40 bg-amber-700/10 px-1.5 py-0.5 text-[10px] font-semibold text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
      : 'rounded border border-stone-200 px-1.5 py-0.5 text-[10px] text-stone-500 dark:border-stone-700 dark:text-stone-400';
  }

  const moreClasses =
    'rounded border border-dashed border-stone-300 px-1.5 py-0.5 text-[10px] font-semibold text-stone-500 hover:border-stone-400 hover:text-stone-700 dark:border-stone-600 dark:text-stone-400 dark:hover:border-stone-500 dark:hover:text-stone-200';

  // Rows are themselves a clickable container (opens the paper) — stop the
  // +N control's click from bubbling up and opening it too.
  function onMoreClick(e: MouseEvent) {
    e.stopPropagation();
    expanded = !expanded;
  }
</script>

{#if paper.projects.length || paper.tags.length}
  <div class="mt-1.5 flex flex-wrap items-center gap-1">
    {#each paper.projects as project (project.id)}
      <span class={badgeClasses}>
        <Bookmark size={9} />
        {project.name}
      </span>
    {/each}
    {#each visibleTags as tag (tag.id)}
      <span class={chipClasses(isHit(tag.name))}>{tag.name}</span>
    {/each}
    {#if hiddenCount > 0}
      <button type="button" onclick={onMoreClick} class={moreClasses}>
        +{hiddenCount}
      </button>
    {/if}
  </div>
{/if}
