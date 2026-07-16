<script lang="ts">
  import { onMount, type Component } from 'svelte';
  import { Spring } from 'svelte/motion';
  import { fly, slide } from 'svelte/transition';
  import ChatPanel from './components/ChatPanel.svelte';
  import CommandPalette from './components/CommandPalette.svelte';
  import IdentifyModal from './components/IdentifyModal.svelte';
  import ImportModal from './components/ImportModal.svelte';
  import InfoPanel from './components/InfoPanel.svelte';
  import LibraryPane from './components/LibraryPane.svelte';
  import PaperContextMenu from './components/PaperContextMenu.svelte';
  import TabBar from './components/TabBar.svelte';
  import Toaster from './components/Toaster.svelte';
  import TopBar from './components/TopBar.svelte';
  import TranslatePopover from './components/TranslatePopover.svelte';
  import Welcome from './components/Welcome.svelte';
  import { chat, loadChatModels, loadThread } from './lib/chat.svelte';
  import { DUR, dur, prefersReducedMotion, SPRINGS } from './lib/motion';
  import { handleKeydown } from './lib/shortcuts';
  import { syncTranslateModeFromSettings } from './lib/translate.svelte';
  import {
    identifyState,
    initInfo,
    initTheme,
    loadPapers,
    loadProjects,
    loadSearchStatus,
    loadSettings,
    loadStats,
    ui,
    viewer,
  } from './lib/state.svelte';

  onMount(() => {
    initTheme();
    initInfo();
    loadStats();
    loadProjects();
    loadPapers();
    loadSearchStatus();
    void loadSettings().then(syncTranslateModeFromSettings);
    loadChatModels();
  });

  const PANE_W = 304;
  const paneW = new Spring(PANE_W, SPRINGS.pane);
  let peek = $state(false);
  const paneHidden = $derived(!ui.sidebarOpen || ui.zen);
  $effect(() => {
    const target = paneHidden ? 0 : PANE_W;
    if (import.meta.env.MODE === 'test' || prefersReducedMotion()) {
      paneW.set(target, { instant: true });
    } else {
      paneW.target = target;
    }
  });
  $effect(() => {
    if (!paneHidden) peek = false;
  });
  // The chat thread follows the active paper while the panel is open.
  $effect(() => {
    if (chat.open && viewer.activeId) void loadThread(viewer.activeId);
  });

  // The PDF reader pulls in the entire @embedpdf/PDFium subtree — by far the
  // heaviest part of the bundle. Load it lazily (its own chunk) the first time
  // a paper is opened, so the library/search view no longer pays for it up
  // front. Once loaded it stays resolved for the session.
  let PdfViewer = $state<Component | null>(null);
  $effect(() => {
    if (viewer.activeId !== null && !PdfViewer) {
      void import('./components/PdfViewer.svelte').then((m) => {
        PdfViewer = m.default;
      });
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex h-full flex-col bg-paper text-ink dark:bg-night dark:text-stone-100">
  {#if !ui.zen}
    <div transition:slide={{ duration: dur(DUR.base) }}>
      <TopBar />
    </div>
  {/if}
  <div class="relative flex min-h-0 flex-1">
    <div class="relative min-h-0 shrink-0 overflow-hidden" style={`width:${paneW.current}px`} inert={paneHidden}>
      <div class="absolute inset-y-0 left-0 w-[304px]"><LibraryPane /></div>
    </div>
    {#if paneHidden}
      <!-- Edge peek: hover the left edge to overlay the list without expanding it. -->
      <div class="absolute inset-y-0 left-0 z-30 w-2" onmouseenter={() => (peek = true)} role="presentation"></div>
      {#if peek}
        <div
          transition:fly={{ x: -24, duration: dur(DUR.base) }}
          onmouseleave={() => (peek = false)}
          role="presentation"
          class="absolute inset-y-0 left-0 z-40 shadow-2xl"
        >
          <LibraryPane />
        </div>
      {/if}
    {/if}
    <main class="flex min-h-0 min-w-0 flex-1 flex-col">
      {#if !ui.zen}
        <div transition:slide={{ duration: dur(DUR.base) }}>
          <TabBar />
        </div>
      {/if}
      <div class="flex min-h-0 flex-1">
        <!-- Reader column is hidden (not unmounted) while the Library view is
             active, so returning to an open paper doesn't rebuild the subtree.
             (The viewer itself remounts per paper, so scroll isn't preserved.) -->
        <div class={`relative min-h-0 min-w-0 flex-1 ${viewer.activeId === null ? 'hidden' : 'flex'}`}>
          {#if PdfViewer}
            <PdfViewer />
          {:else if viewer.activeId !== null}
            <div class="flex flex-1 items-center justify-center text-sm text-stone-400 dark:text-stone-500">
              Loading reader…
            </div>
          {/if}
          {#if viewer.infoOpen && viewer.activeId}
            <!-- Non-interactive recede veil: the PDF stays scrollable underneath. -->
            <div class="pointer-events-none absolute inset-0 z-10 bg-ink/5 dark:bg-black/25" aria-hidden="true"></div>
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
          {#if chat.open}<ChatPanel />{/if}
        </div>
        {#if viewer.activeId === null}
          <Welcome />
        {/if}
      </div>
    </main>
  </div>
</div>
{#if ui.importOpen}<ImportModal />{/if}
{#if identifyState.open}<IdentifyModal />{/if}
{#if ui.paletteOpen}<CommandPalette />{/if}
<PaperContextMenu />
<TranslatePopover />
<Toaster />
