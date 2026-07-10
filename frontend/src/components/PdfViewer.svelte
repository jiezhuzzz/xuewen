<script lang="ts">
  import { pdfUrl } from '../lib/api';
  import { viewer } from '../lib/state.svelte';
</script>

<div class="relative min-h-0 flex-1 bg-stone-100 dark:bg-stone-950">
  {#each viewer.tabs as tab (tab.id)}
    <iframe
      title={tab.title}
      src={pdfUrl(tab.id)}
      class={`absolute inset-0 h-full w-full border-0 ${tab.id === viewer.activeId ? '' : 'hidden'}`}
    ></iframe>
  {/each}
</div>
