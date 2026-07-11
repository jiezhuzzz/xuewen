<script lang="ts">
  import { pdfUrl } from '../lib/api';
  import { viewer } from '../lib/state.svelte';

  let shell = $state<HTMLDivElement | null>(null);

  // The browser's native PDF viewer runs the <iframe> out of process; once it
  // takes keyboard focus it swallows every keystroke, so the window-level
  // shortcuts (z, x, i, c, …) silently stop firing. When focus lands in our
  // PDF iframe, hand it straight back to the reader shell so those shortcuts
  // keep working. Mouse/trackpad scroll, zoom and text selection don't need
  // keyboard focus, so they are unaffected; the trade-off is that the PDF
  // plugin's own keyboard (Ctrl/⌘F find, arrow-key scroll) is disabled.
  function reclaimFocus(): void {
    // On blur the parent's activeElement hasn't settled onto the iframe yet;
    // defer a tick, then only reclaim if focus really went into our iframe.
    setTimeout(() => {
      const el = document.activeElement;
      if (el instanceof HTMLIFrameElement && shell?.contains(el)) {
        shell.focus({ preventScroll: true });
      }
    }, 0);
  }
</script>

<svelte:window onblur={reclaimFocus} />

<div
  bind:this={shell}
  tabindex="-1"
  class="relative min-h-0 flex-1 bg-stone-100 outline-none dark:bg-stone-950"
>
  {#each viewer.tabs as tab (tab.id)}
    <iframe
      title={tab.title}
      src={pdfUrl(tab.id)}
      class={`absolute inset-0 h-full w-full border-0 ${tab.id === viewer.activeId ? '' : 'hidden'}`}
    ></iframe>
  {/each}
</div>
