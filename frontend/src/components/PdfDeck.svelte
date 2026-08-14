<script lang="ts">
  import { onDestroy } from 'svelte';
  import { viewer } from '../lib/state.svelte';
  import { pdfUrl } from '../lib/api';
  import { useDocumentManagerCapability } from '@embedpdf/plugin-document-manager/svelte';
  import { planOpens, reconcileDocuments } from '../lib/pdfDeck';
  import { runWhenIdle } from '../lib/idle';
  import PdfTab from './PdfTab.svelte';
  import CitationPopover from './CitationPopover.svelte';

  // Runs inside <EmbedPDF>, so the document-manager capability resolves against
  // the shared registry. Each open paper tab becomes a document here.
  const dm = useDocumentManagerCapability();
  type DocumentManager = NonNullable<typeof dm.provides>;

  // Documents we've asked the manager to open. Plain (non-reactive) set used to
  // diff against `viewer.tabs` so we open/close each document exactly once.
  const opened = new Set<string>();

  // Background tabs waiting their turn — see planOpens for why they wait.
  const pending: string[] = [];
  let draining = false;
  let cancelIdle: (() => void) | null = null;

  // openDocumentUrl's task rejects if the manager's maxDocuments cap is hit (or
  // the document errors before an id is assigned). Roll back so a later effect
  // run (the next tab change) retries the open instead of leaving the tab
  // stranded with no document.
  function open(cap: DocumentManager, id: string, done: () => void): void {
    opened.add(id);
    cap.openDocumentUrl({ url: pdfUrl(id), documentId: id, autoActivate: false }).wait(
      // The outer task resolves SYNCHRONOUSLY, carrying the real load task
      // nested inside it (plugin-document-manager's openDocumentUrl:169), so
      // waiting on the outer one reports "loaded" before a single byte has been
      // read — which would let background tabs start immediately, defeating the
      // whole point of deferring them. Chain on the inner task instead.
      ({ task }) => task.wait(done, done),
      () => {
        opened.delete(id);
        done();
      },
    );
  }

  /// Open one queued background document per idle moment, in tab order.
  /// Sequential and idle-gated, not a burst: each open is PDFium work on the
  /// shared engine lane plus a page-raster storm on the main thread, and firing
  /// several at once is exactly what jammed the renderer on a restored session.
  function drain(cap: DocumentManager): void {
    if (draining || cancelIdle) return;
    const id = pending.shift();
    if (id === undefined) return;
    draining = true;
    cancelIdle = runWhenIdle(() => {
      cancelIdle = null;
      // It may have been closed while queued, or switched to and therefore
      // already opened ahead of its turn by the effect below.
      if (!viewer.tabs.some((t) => t.id === id) || opened.has(id)) {
        draining = false;
        drain(cap);
        return;
      }
      open(cap, id, () => {
        draining = false;
        drain(cap);
      });
    });
  }

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
    // Closed first: it frees slots against maxDocuments before we ask for more.
    for (const id of toClose) {
      opened.delete(id);
      cap.closeDocument(id);
    }

    const { now, deferred } = planOpens(toOpen, viewer.activeId);
    for (const id of deferred) {
      if (!pending.includes(id)) pending.push(id);
    }
    // Switching to a tab whose open was still queued re-runs this effect with
    // that tab active, so it lands in `now` and opens at once — its stale queue
    // entry is skipped when the drain reaches it.
    if (now.length === 0) {
      drain(cap);
      return;
    }
    for (const id of now) {
      // Background tabs start only once the visible document is parsed, and
      // then only at idle. A failed open still drains, or one broken PDF would
      // strand every other tab unopened.
      open(cap, id, () => drain(cap));
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

  onDestroy(() => {
    cancelIdle?.();
    cancelIdle = null;
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
