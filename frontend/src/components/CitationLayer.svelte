<script lang="ts">
  import type { CitationData } from '../lib/citations';
  import type { PaperSummary } from '../lib/types';
  import { showCitation, hideCitationSoon } from '../lib/citationState.svelte';

  let {
    pageIndex,
    pageWidthPt,
    pageHeightPt,
    data,
    matches,
  }: {
    pageIndex: number;
    pageWidthPt: number;
    pageHeightPt: number;
    data: CitationData;
    matches: Map<number, PaperSummary>;
  } = $props();

  const markers = $derived(data.markers.filter((m) => m.pageIndex === pageIndex));

  function enter(e: PointerEvent, refIndex: number) {
    const reference = data.references[refIndex];
    if (!reference) return;
    showCitation({
      reference,
      matchedPaper: matches.get(refIndex) ?? null,
      screenX: e.clientX,
      screenY: e.clientY,
    });
  }
</script>

<!-- Absolutely-positioned, non-blocking layer filling the page box. Each hit-box
     is a % of the page so it tracks zoom automatically. pointer-events only on
     the boxes so text selection underneath still works. -->
<div class="pointer-events-none absolute inset-0">
  {#each markers as m (`${m.x},${m.y}`)}
    <span
      role="link"
      tabindex="0"
      aria-label="Citation reference {m.refIndex + 1}"
      onpointerenter={(e) => enter(e, m.refIndex)}
      onpointerleave={hideCitationSoon}
      style:left="{(m.x / pageWidthPt) * 100}%"
      style:top="{(m.y / pageHeightPt) * 100}%"
      style:width="{(m.width / pageWidthPt) * 100}%"
      style:height="{(m.height / pageHeightPt) * 100}%"
      class="pointer-events-auto absolute cursor-help rounded-sm border-b-2 border-dotted border-amber-600/55 bg-amber-400/0 hover:border-amber-600/0 hover:bg-amber-400/25 dark:border-amber-500/60"
    ></span>
  {/each}
</div>
