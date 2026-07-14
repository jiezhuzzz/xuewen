<script lang="ts">
  import { ExternalLink, BookOpen } from 'lucide-svelte';
  import { citationHover, cancelHideCitation, hideCitationSoon } from '../lib/citationState.svelte';
  import { openTab } from '../lib/state.svelte';
  import { abbreviateVenue } from '../lib/venue';
  import { authorLine, refLinks, titleCase } from '../lib/refFormat';

  const c = $derived(citationHover.current);
  const s = $derived(c?.reference.structured ?? null);
  const links = $derived(c ? refLinks(s, c.reference.externalUrl) : []);

  // Keep the popover on screen. It normally sits ABOVE the marker; near the top
  // of the viewport that would clip off-screen (looked like "no popup"), so flip
  // it BELOW. Also clamp horizontally so it never runs off the right edge.
  const MARGIN = 8;
  const MAX_W = 384; // Tailwind max-w-sm
  const vw = $derived(typeof window === 'undefined' ? 1280 : window.innerWidth);
  const below = $derived((c?.screenY ?? 0) < 220);
  const left = $derived(Math.max(MARGIN, Math.min(c?.screenX ?? 0, vw - MAX_W - MARGIN)));
  const top = $derived(below ? (c?.screenY ?? 0) + 18 : (c?.screenY ?? 0) - 8);

  function open() {
    if (c?.matchedPaper) openTab(c.matchedPaper);
    citationHover.current = null;
  }
</script>

{#if c}
  <div
    role="tooltip"
    onpointerenter={cancelHideCitation}
    onpointerleave={hideCitationSoon}
    style:left="{left}px"
    style:top="{top}px"
    style:transform={below ? 'none' : 'translateY(-100%)'}
    class="pointer-events-auto fixed z-50 max-w-sm rounded-xl border border-stone-200 bg-paper p-3 text-[12.5px] shadow-2xl dark:border-stone-800 dark:bg-soot"
  >
    {#if s?.title}
      <p class="font-medium leading-snug text-ink dark:text-stone-100">{titleCase(s.title)}</p>
      {#if s.authors.length > 0}
        <p class="mt-0.5 leading-snug text-stone-500 dark:text-stone-400">{authorLine(s.authors)}</p>
      {/if}
      {#if s.venue || s.year}
        <p class="mt-0.5 text-xs text-stone-500 dark:text-stone-400">
          {[abbreviateVenue(s.venue), s.year].filter(Boolean).join(' · ')}
        </p>
      {/if}
    {:else}
      <p class="font-serif leading-relaxed text-stone-700 dark:text-stone-300">{c.reference.rawText}</p>
    {/if}
    <div class="mt-2 flex items-center gap-3">
      {#if c.matchedPaper}
        <button
          type="button"
          onclick={open}
          class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
        >
          <BookOpen size={13} /> Open in library
        </button>
      {/if}
      {#each links as l (l.href)}
        <a
          href={l.href}
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center gap-1 text-xs text-stone-500 hover:text-ink dark:text-stone-400"
        >
          <ExternalLink size={12} /> {l.label}
        </a>
      {/each}
    </div>
  </div>
{/if}
