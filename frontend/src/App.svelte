<script lang="ts">
  import { onMount } from 'svelte';
  import EmptyState from './components/EmptyState.svelte';
  import IdentifyModal from './components/IdentifyModal.svelte';
  import ImportModal from './components/ImportModal.svelte';
  import InfoPanel from './components/InfoPanel.svelte';
  import PdfViewer from './components/PdfViewer.svelte';
  import ProjectsModal from './components/ProjectsModal.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import TabBar from './components/TabBar.svelte';
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

<div class="flex h-full flex-col bg-slate-50 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
  <TopBar />
  <div class="flex min-h-0 flex-1">
    {#if ui.sidebarOpen}<Sidebar />{/if}
    <main class="flex min-h-0 flex-1 flex-col">
      {#if viewer.tabs.length === 0}
        <EmptyState />
      {:else}
        <TabBar />
        <div class="flex min-h-0 flex-1">
          <PdfViewer />
          {#if viewer.infoOpen && viewer.activeId}
            {#key viewer.activeId}
              <InfoPanel id={viewer.activeId} />
            {/key}
          {/if}
        </div>
      {/if}
    </main>
  </div>
</div>
{#if ui.importOpen}<ImportModal />{/if}
{#if identifyState.open}<IdentifyModal />{/if}
{#if ui.projectsOpen}<ProjectsModal />{/if}
