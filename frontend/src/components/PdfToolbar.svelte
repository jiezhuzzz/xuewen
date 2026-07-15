<script lang="ts">
  import {
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Info,
    Maximize2,
    Minimize2,
    PanelLeft,
    Search,
    ZoomIn,
    ZoomOut,
  } from 'lucide-svelte';
  import { useZoom } from '@embedpdf/plugin-zoom/svelte';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import { DUR, dur, EASE } from '../lib/motion';
  import { toggleInfo, toggleZen, ui, viewer } from '../lib/state.svelte';
  import { reader, setFind, toggleSidebar } from '../lib/readerState.svelte';
  import { HIDE_DELAY_MS, HOT_ZONE_PX, holdVisible, toolbarVisible, type ToolbarHold } from '../lib/zenToolbar';
  import { clampPage } from '../lib/pageNav';
  import { formatScale, isActivePreset, ZOOM_PRESETS } from '../lib/zoomPresets';

  let { documentId }: { documentId: string } = $props();

  const zoom = useZoom(() => documentId);
  const scroll = useScroll(() => documentId);

  // --- page-number input: mirrors currentPage except while being edited ---
  let pageText = $state('1');
  let pageEditing = $state(false);
  $effect(() => {
    if (!pageEditing) pageText = String(scroll.state.currentPage);
  });
  function commitPage(target: HTMLInputElement): void {
    const n = clampPage(pageText, scroll.state.totalPages);
    if (n !== null) scroll.provides?.scrollToPage({ pageNumber: n });
    target.blur();
  }

  // --- zoom preset menu ---
  let zoomMenuOpen = $state(false);
  let zoomMenuWrap: HTMLDivElement | undefined = $state();
  function onWindowPointerDown(e: PointerEvent): void {
    if (zoomMenuOpen && !(e.target instanceof Node && zoomMenuWrap?.contains(e.target))) {
      zoomMenuOpen = false;
    }
  }

  // --- zen auto-hide. Decision logic lives in lib/zenToolbar.ts; this owns
  // the timer and the DOM signals feeding it. The pill hides via opacity
  // (not {#if}) so its box survives for offsetParent math and transitions.
  let hotZone = $state(false);
  let pointerOver = $state(false);
  let focusWithin = $state(false);
  let idleExpired = $state(false);
  let pillEl: HTMLDivElement | undefined = $state();

  // A stale hot-zone from a previous zen session must not hold the pill
  // visible on re-entry — onWindowMove only updates hotZone while zen is
  // active, so without this it freezes at its last value across exit/re-entry.
  $effect(() => {
    if (!ui.zen) hotZone = false;
  });

  const hold = $derived<ToolbarHold>({
    zen: ui.zen,
    hotZone,
    pointerOver,
    focusWithin,
    findOpen: !!reader.find[documentId],
    // Any toolbar-local interaction (editing the page number, an open zoom
    // menu) holds the pill visible; zenToolbar.ts doesn't care which.
    pageEditing: pageEditing || zoomMenuOpen,
  });
  const visible = $derived(toolbarVisible(hold, idleExpired));

  // Any hold cancels the countdown and re-arms visibility; once every hold
  // drops in zen, the countdown starts.
  $effect(() => {
    if (holdVisible(hold)) {
      idleExpired = false;
      return;
    }
    const t = setTimeout(() => (idleExpired = true), HIDE_DELAY_MS);
    return () => clearTimeout(t);
  });

  // Hot-zone tracking is window-level so it works while the pill is faded
  // out. Only the active tab's toolbar reacts (hidden tabs stay mounted).
  function onWindowMove(e: PointerEvent): void {
    if (!ui.zen || viewer.activeId !== documentId) return;
    const host = pillEl?.offsetParent;
    if (!host) return;
    const top = host.getBoundingClientRect().top;
    hotZone = e.clientY >= top && e.clientY - top < HOT_ZONE_PX;
  }

  const title = $derived(viewer.tabs.find((t) => t.id === documentId)?.title ?? '');
  const panel = $derived(reader.panel[documentId] ?? null);

  const btn =
    'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-40 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800';
  const activeBtn = 'rounded-lg p-1.5 bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
</script>

<svelte:window onpointermove={onWindowMove} onpointerdown={onWindowPointerDown} />

<!-- svelte-ignore a11y_interactive_supports_focus -- every control inside
     the pill is individually tabbable via normal document tab order; the
     toolbar container itself is not a tab stop. A roving-tabindex pass for
     arrow-key navigation between controls is a filed follow-up, not done
     here. -->
<div
  bind:this={pillEl}
  role="toolbar"
  aria-label="PDF controls"
  onpointerenter={() => (pointerOver = true)}
  onpointerleave={() => (pointerOver = false)}
  onfocusin={() => (focusWithin = true)}
  onfocusout={() => (focusWithin = false)}
  style:transition="opacity {dur(DUR.base)}ms {EASE}"
  class={`absolute left-1/2 top-3 z-20 flex -translate-x-1/2 items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90 ${
    visible ? 'opacity-100' : 'pointer-events-none opacity-0'
  }`}
>
  {#if ui.zen}
    <span class="max-w-48 truncate px-1 font-serif text-sm text-ink dark:text-stone-100">{title}</span>
    <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>
  {/if}

  <button
    type="button"
    class={panel ? activeBtn : btn}
    aria-label="Toggle sidebar"
    aria-expanded={panel !== null}
    title="Sidebar"
    onclick={() => toggleSidebar(documentId)}
  >
    <PanelLeft size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button
    type="button"
    class={btn}
    aria-label="Previous page"
    disabled={scroll.state.currentPage <= 1}
    onclick={() => scroll.provides?.scrollToPreviousPage()}
  >
    <ChevronLeft size={16} />
  </button>
  <input
    class="w-9 rounded-md border border-transparent bg-transparent text-center text-sm text-stone-700 focus:border-stone-300 focus:outline-none dark:text-stone-200 dark:focus:border-stone-700"
    aria-label="Page number"
    bind:value={pageText}
    onfocus={(e) => {
      pageEditing = true;
      e.currentTarget.select();
    }}
    onblur={() => (pageEditing = false)}
    onkeydown={(e) => {
      if (e.key === 'Enter') commitPage(e.currentTarget);
      else if (e.key === 'Escape') {
        e.stopPropagation(); // the global cascade must not see this
        pageText = String(scroll.state.currentPage);
        e.currentTarget.blur();
      }
    }}
  />
  <span class="text-sm text-stone-500 dark:text-stone-400">/ {scroll.state.totalPages}</span>
  <button
    type="button"
    class={btn}
    aria-label="Next page"
    disabled={scroll.state.currentPage >= scroll.state.totalPages}
    onclick={() => scroll.provides?.scrollToNextPage()}
  >
    <ChevronRight size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button type="button" class={btn} aria-label="Zoom out" onclick={() => zoom.provides?.zoomOut()}>
    <ZoomOut size={16} />
  </button>
  <div class="relative" bind:this={zoomMenuWrap}>
    <button
      type="button"
      class={`${zoomMenuOpen ? activeBtn : btn} flex items-center gap-0.5 text-sm tabular-nums`}
      aria-label="Zoom level"
      aria-expanded={zoomMenuOpen}
      onclick={() => (zoomMenuOpen = !zoomMenuOpen)}
    >
      {formatScale(zoom.state.currentZoomLevel)}
      <ChevronDown size={12} />
    </button>
    {#if zoomMenuOpen}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the
           keydown only contains Escape inside the menu (same pattern as the
           find bar); every item is a real button. -->
      <div
        role="menu"
        aria-label="Zoom presets"
        class="absolute left-1/2 top-full z-30 mt-1.5 w-28 -translate-x-1/2 rounded-xl border border-stone-200 bg-paper/95 p-1 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95"
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation(); // the global cascade must not see this
            zoomMenuOpen = false;
          }
        }}
      >
        {#each ZOOM_PRESETS as p (p.label)}
          <button
            type="button"
            role="menuitem"
            class={`block w-full rounded-lg px-2 py-1 text-left text-xs ${
              isActivePreset(p, zoom.state.currentZoomLevel)
                ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
                : 'text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800'
            }`}
            onclick={() => {
              zoom.provides?.requestZoom(p.level);
              zoomMenuOpen = false;
            }}
          >
            {p.label}
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <button type="button" class={btn} aria-label="Zoom in" onclick={() => zoom.provides?.zoomIn()}>
    <ZoomIn size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button
    type="button"
    class={reader.find[documentId] ? activeBtn : btn}
    aria-label="Find in document"
    aria-expanded={!!reader.find[documentId]}
    title="Find (⌘F)"
    onclick={() => setFind(documentId)}
  >
    <Search size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button
    type="button"
    class={btn}
    aria-label={ui.zen ? 'Exit zen mode' : 'Zen mode'}
    title="Zen mode (z)"
    onclick={toggleZen}
  >
    {#if ui.zen}<Minimize2 size={16} />{:else}<Maximize2 size={16} />{/if}
  </button>
  <button
    type="button"
    class={viewer.infoOpen ? activeBtn : btn}
    aria-label="Toggle info"
    aria-expanded={viewer.infoOpen}
    title="Info (i)"
    onclick={toggleInfo}
  >
    <Info size={16} />
  </button>
</div>
