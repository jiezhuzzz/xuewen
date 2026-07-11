<script lang="ts">
  import { onMount } from 'svelte';
  import { Spring } from 'svelte/motion';
  import { fly, slide } from 'svelte/transition';
  import ChatBubble from './components/ChatBubble.svelte';
  import ChatPanel from './components/ChatPanel.svelte';
  import CommandPalette from './components/CommandPalette.svelte';
  import IdentifyModal from './components/IdentifyModal.svelte';
  import ImportModal from './components/ImportModal.svelte';
  import InfoPanel from './components/InfoPanel.svelte';
  import LibraryPane from './components/LibraryPane.svelte';
  import PdfViewer from './components/PdfViewer.svelte';
  import ProjectsModal from './components/ProjectsModal.svelte';
  import TabBar from './components/TabBar.svelte';
  import Toaster from './components/Toaster.svelte';
  import TopBar from './components/TopBar.svelte';
  import Welcome from './components/Welcome.svelte';
  import ZenPill from './components/ZenPill.svelte';
  import { chat, loadChatModels, loadThread } from './lib/chat.svelte';
  import { DUR, dur, prefersReducedMotion, SPRINGS } from './lib/motion';
  import { handleKeydown } from './lib/shortcuts';
  import {
    identifyState,
    initInfo,
    initTheme,
    loadPapers,
    loadProjects,
    loadSearchStatus,
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
        <!-- PdfViewer stays mounted while home is active so iframe scroll
             positions survive a trip to the Library. -->
        <div class={`relative min-h-0 min-w-0 flex-1 ${viewer.activeId === null ? 'hidden' : 'flex'}`}>
          <PdfViewer />
          {#if viewer.infoOpen && viewer.activeId}
            <!-- Non-interactive recede veil: the PDF stays scrollable underneath. -->
            <div class="pointer-events-none absolute inset-0 z-10 bg-ink/5 dark:bg-black/25" aria-hidden="true"></div>
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
          {#if chat.available && !chat.open}<ChatBubble />{/if}
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
{#if ui.projectsOpen}<ProjectsModal />{/if}
{#if ui.paletteOpen}<CommandPalette />{/if}
{#if ui.zen}<ZenPill />{/if}
<Toaster />
