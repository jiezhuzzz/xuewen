<script lang="ts">
  import { onDestroy } from 'svelte';
  import { copyText } from '../lib/clipboard';
  import { viewer } from '../lib/tabs.svelte';
  import { pdfUrl } from '../lib/api';
  import { useDocumentManagerCapability } from '@embedpdf/plugin-document-manager/svelte';
  import { useSelectionCapability } from '@embedpdf/plugin-selection/svelte';
  import { useAnnotationCapability } from '@embedpdf/plugin-annotation/svelte';
  import { applyToolDefaults } from '../lib/annotationAdapter';
  import { annotationTools } from '../lib/annotationState.svelte';
  import { colorHex } from '../lib/annotationPalette';
  import { DocumentOpenError, openDocumentFully, planOpens, reconcileDocuments } from '../lib/pdfDeck';
  import { createPdfCopy, forgetPdfSelection, registerPdfCopy } from '../lib/pdfCopy';
  import { runWhenIdle } from '../lib/idle';
  import { toast } from '../lib/toasts.svelte';
  import PdfTab from './PdfTab.svelte';
  import CitationPopover from './CitationPopover.svelte';

  // Runs inside <EmbedPDF>, so the document-manager capability resolves against
  // the shared registry. Each open paper tab becomes a document here.
  const dm = useDocumentManagerCapability();
  type DocumentManager = NonNullable<typeof dm.provides>;

  const selectionCap = useSelectionCapability();

  // ⌘C support for the reader (see lib/pdfCopy.ts for why the app has to do
  // this itself). It belongs HERE, not in PdfPages: the selection capability is
  // registry-wide, and PdfDeck is mounted exactly once, whereas PdfPages is
  // mounted once per open tab and kept alive behind visibility:hidden — up to
  // maxDocuments of them would each subscribe and each fire a redundant PDFium
  // round-trip for one selection.
  $effect(() => {
    const cap = selectionCap.provides;
    if (!cap) return;
    const copier = createPdfCopy({
      // An adapter rather than `cap` itself: the capability's event hooks are
      // typed EventHook<T>, a union of two call signatures, which does not
      // assign to the plain methods SelectionLike declares.
      selection: {
        getSelectedText: (id) => cap.getSelectedText(id),
        onBeginSelection: (h) => cap.onBeginSelection(h),
        onSelectionChange: (h) => cap.onSelectionChange(h),
        onEndSelection: (h) => cap.onEndSelection(h),
      },
      activeDocumentId: () => viewer.activeId,
      copy: copyText,
      clearNativeSelection: () => document.getSelection()?.removeAllRanges(),
      onError: (message) => toast('error', message),
    });
    registerPdfCopy(copier);
    return () => {
      registerPdfCopy(null);
      copier.destroy();
    };
  });

  // The palette color → tool defaults push. Tool defaults are GLOBAL in the
  // annotation plugin, so this lives here for the same reason the ⌘C wiring
  // does: PdfDeck mounts exactly once, whereas each open tab's AnnotationTools
  // would redundantly re-push the same five defaults per color change.
  const annotationCap = useAnnotationCapability();
  $effect(() => {
    const cap = annotationCap.provides;
    if (!cap) return;
    applyToolDefaults(cap, colorHex(annotationTools.color));
  });

  // Documents we've asked the manager to open. Plain (non-reactive) set used to
  // diff against `viewer.tabs` so we open/close each document exactly once.
  const opened = new Set<string>();

  // Background tabs waiting their turn — see planOpens for why they wait.
  const pending: string[] = [];
  let draining = false;
  let cancelIdle: (() => void) | null = null;

  // openDocumentFully (lib/pdfDeck.ts) owns the outer-task-resolves-
  // synchronously trap and rejects with the failing phase. Only an 'open'
  // failure (maxDocuments cap hit, or the document errored before an id was
  // assigned) rolls back, so a later effect run (the next tab change) retries
  // instead of leaving the tab stranded with no document; a 'load' failure
  // keeps the id in `opened` — retrying a broken PDF on every tab change
  // would loop forever.
  function open(cap: DocumentManager, id: string, done: () => void): void {
    opened.add(id);
    openDocumentFully(cap, { url: pdfUrl(id), documentId: id }).then(
      () => done(),
      (e) => {
        if (e instanceof DocumentOpenError && e.phase === 'open') opened.delete(id);
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
      // The plugin emits no selection-change on close, so this is the only
      // thing bounding the copy cache to the set of open tabs.
      forgetPdfSelection(id);
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
