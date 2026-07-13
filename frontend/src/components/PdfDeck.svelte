<script lang="ts">
  import { viewer } from '../lib/state.svelte';
  import { pdfUrl } from '../lib/api';
  import { useDocumentManagerCapability } from '@embedpdf/plugin-document-manager/svelte';
  import { reconcileDocuments } from '../lib/pdfDeck';
  import PdfTab from './PdfTab.svelte';
  import CitationPopover from './CitationPopover.svelte';

  // Runs inside <EmbedPDF>, so the document-manager capability resolves against
  // the shared registry. Each open paper tab becomes a document here.
  const dm = useDocumentManagerCapability();

  // Documents we've asked the manager to open. Plain (non-reactive) set used to
  // diff against `viewer.tabs` so we open/close each document exactly once.
  const opened = new Set<string>();

  // Open a document for every new tab and close documents whose tab is gone.
  // Per-document scroll/zoom lives in the plugin store keyed by documentId, so
  // switching the active document (below) preserves each tab's position.
  $effect(() => {
    const cap = dm.provides;
    if (!cap) return;
    const { toOpen, toClose } = reconcileDocuments(
      opened,
      viewer.tabs.map((t) => t.id),
    );
    for (const id of toOpen) {
      opened.add(id);
      // openDocumentUrl's task rejects if the manager's maxDocuments cap is
      // hit (or the document errors before an id is assigned). Roll back so
      // a later effect run (the next tab change) retries the open instead of
      // leaving the tab stranded with no document.
      cap.openDocumentUrl({ url: pdfUrl(id), documentId: id, autoActivate: false }).wait(
        () => {},
        () => opened.delete(id),
      );
    }
    for (const id of toClose) {
      opened.delete(id);
      cap.closeDocument(id);
    }
  });

  // Keep the manager's active document in sync with the active tab.
  // setActiveDocument throws if the document isn't open yet (e.g. still
  // loading, or its open was rejected above), so guard with isDocumentOpen.
  $effect(() => {
    const cap = dm.provides;
    if (cap && viewer.activeId && cap.isDocumentOpen(viewer.activeId)) {
      cap.setActiveDocument(viewer.activeId);
    }
  });
</script>

<!-- One persistent PdfTab per open tab, hidden unless active: switching tabs is
     a show/hide, never a remount, so scroll/page/zoom survive the switch. -->
<div class="relative h-full w-full">
  {#each viewer.tabs as tab (tab.id)}
    <PdfTab id={tab.id} active={tab.id === viewer.activeId} />
  {/each}
  <CitationPopover />
</div>
