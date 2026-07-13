<script lang="ts">
  import { theme, viewer } from '../lib/state.svelte';
  import { themePreference } from '../lib/pdfEngine';
  import PdfTab from './PdfTab.svelte';

  // Effective dark for `system` mode. Reactive to explicit theme changes; the
  // OS-follow case updates on the media-query event below. Resolved once here
  // and passed to every tab so they stay in sync.
  let systemDark = $state(
    typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches,
  );
  $effect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  });

  const preference = $derived(themePreference(theme.mode, systemDark));
</script>

<!-- One persistent PdfTab per open tab, hidden unless active: switching tabs is
     a show/hide, never a remount, so scroll/page/zoom survive the switch. -->
<div class="relative min-h-0 flex-1 bg-stone-100 dark:bg-stone-950">
  {#each viewer.tabs as tab (tab.id)}
    <PdfTab id={tab.id} {preference} active={tab.id === viewer.activeId} />
  {/each}
</div>
