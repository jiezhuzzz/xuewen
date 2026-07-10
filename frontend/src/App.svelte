<script lang="ts">
  import { onMount } from 'svelte';
  import DetailView from './components/DetailView.svelte';
  import IdentifyModal from './components/IdentifyModal.svelte';
  import ImportModal from './components/ImportModal.svelte';
  import InfoPanel from './components/InfoPanel.svelte';
  import LibraryPane from './components/LibraryPane.svelte';
  import PdfViewer from './components/PdfViewer.svelte';
  import ProjectsModal from './components/ProjectsModal.svelte';
  import TabBar from './components/TabBar.svelte';
  import Toaster from './components/Toaster.svelte';
  import TopBar from './components/TopBar.svelte';
  import {
    identifyState,
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
    loadStats();
    loadProjects();
    loadPapers();
    loadSearchStatus();
  });
</script>

<div class="flex h-full flex-col bg-paper text-ink dark:bg-night dark:text-stone-100">
  <TopBar />
  <div class="flex min-h-0 flex-1">
    {#if ui.sidebarOpen}<LibraryPane />{/if}
    <main class="flex min-h-0 min-w-0 flex-1 flex-col">
      <TabBar />
      <div class="flex min-h-0 flex-1">
        <!-- PdfViewer stays mounted while home is active so iframe scroll
             positions survive a trip to the Library. -->
        <div class={`min-h-0 min-w-0 flex-1 ${viewer.activeId === null ? 'hidden' : 'flex'}`}>
          <PdfViewer />
          {#if viewer.infoOpen && viewer.activeId}
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
        </div>
        {#if viewer.activeId === null}
          <DetailView />
        {/if}
      </div>
    </main>
  </div>
</div>
{#if ui.importOpen}<ImportModal />{/if}
{#if identifyState.open}<IdentifyModal />{/if}
{#if ui.projectsOpen}<ProjectsModal />{/if}
<Toaster />
