<script lang="ts">
  import { onMount } from 'svelte';
  import Sidebar from './components/Sidebar.svelte';
  import TopBar from './components/TopBar.svelte';
  import { initTheme, loadPapers, loadStats } from './lib/state.svelte';

  onMount(() => {
    initTheme();
    loadStats();
    loadPapers();
  });
</script>

<div class="flex h-full flex-col bg-slate-50 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
  <TopBar />
  <div class="flex min-h-0 flex-1">
    <Sidebar />
    <main class="grid min-h-0 flex-1 place-items-center text-slate-500 dark:text-slate-400">
      <p>Select a paper to open its PDF.</p>
    </main>
  </div>
</div>
