/// ⌘C over the reader. Copying a PDF selection is entirely this app's job:
/// EmbedPDF binds no keys anywhere, and the only clipboard write in the whole
/// package tree lives in an auto-mounted utility we deliberately don't register
/// (see pdfEngine.ts). The browser can't do it either — a page is an <img>
/// (RenderLayer/TilingLayer) and the selection overlay is empty pointer-events:
/// none <div>s, so the document selection over a page is always collapsed, the
/// copy command stays disabled, and no `copy` event is ever dispatched. There
/// is nothing to intercept; the keystroke has to be handled outright.
///
/// The text therefore has to be in hand BEFORE the keystroke. getSelectedText
/// is a PDFium `getTextSlices` round-trip through the worker, i.e. a full task
/// boundary, and a clipboard write on the far side of one has lost the user
/// gesture that authorized it (WebKit/WKWebView enforces this strictly, and
/// copyText's execCommand fallback needs the same live gesture). So the text is
/// fetched eagerly the moment a selection settles and cached here, and
/// `copySelection` writes it with no await in front of the write.
///
/// Kept DOM-free and dependency-injected so the whole policy is unit-testable;
/// PdfDeck supplies the real capability and `copyText`.

/// The plugin surface this module needs, declared structurally rather than
/// imported as `SelectionCapability`. Two reasons, both load-bearing: the real
/// `PdfTask` aliases a class with private fields, so a `{ toPromise }` fake can
/// never satisfy it and `npm run check` fails on the test (the same reason
/// loadCitations.ts declares `EngineLike`); and the capability's event hooks are
/// typed `EventHook<T>`, a *union* of two call signatures, which does not
/// assign to a plain method under strictFunctionTypes.
export interface SelectionLike {
  getSelectedText(documentId: string): { toPromise(): Promise<string[]> };
  onBeginSelection(handler: (ev: { documentId: string }) => void): () => void;
  onSelectionChange(handler: (ev: { documentId: string; selection: unknown }) => void): () => void;
  onEndSelection(handler: (ev: { documentId: string }) => void): () => void;
}

export interface PdfCopyOptions {
  selection: SelectionLike;
  /// Read at call time, never captured: the active tab changes under us.
  activeDocumentId: () => string | null;
  /// Production: `copyText` from state.svelte.ts (Clipboard API, falling back
  /// to a hidden textarea + execCommand for plain-HTTP `--allow-remote`).
  copy: (text: string) => Promise<void>;
  /// Drops any native DOM selection when a PDF selection starts, so the two can
  /// never both be live and the ⌘C guard stays unambiguous. Production:
  /// `() => document.getSelection()?.removeAllRanges()`.
  clearNativeSelection?: () => void;
  onError?: (message: string) => void;
}

export interface PdfCopy {
  /// Whether ⌘C should be taken over for the active tab right now.
  hasSelection(): boolean;
  copySelection(): Promise<void>;
  /// Drop a closed document's cached text (PdfDeck's close loop).
  forget(documentId: string): void;
  destroy(): void;
}

/// How long the selection must stop changing before we fetch its text.
///
/// This settle timer — not `onEndSelection` — is what makes the feature
/// reliable. The plugin registers its text handler per page
/// (`registerAlways({ scope: { type: 'page', … } })`, plugin-selection
/// 2.14.4 index.js:941) and keeps `dragStarted` in that page's own closure, so
/// `onEnd` fires only when the pointer is released over the very page the drag
/// began on. Releasing in the gutter between pages, past the page margin, over
/// the floating toolbar, or on a second page — i.e. most multi-page and
/// select-to-end-of-paragraph drags — ends with a live, visibly highlighted
/// selection and no end event at all. `onSelectionChange` does fire for every
/// glyph the range moves over (index.js:1262), so a trailing debounce on it
/// catches every one of those cases. `onEndSelection` is kept as the fast path
/// for the common release-on-page drag and for double/triple-click, which
/// emits it with no pointerup at all (index.js:1247).
const SETTLE_MS = 200;

export function createPdfCopy(opts: PdfCopyOptions): PdfCopy {
  const { selection, activeDocumentId, copy, clearNativeSelection, onError } = opts;

  // All three are keyed by documentId, never a single "last selection" slot:
  // selection state lives per document in the plugin and survives a tab switch
  // (only onDocumentClosed tears it down), so a background tab's still-
  // highlighted selection has to stay copyable when the user switches back.
  const cache = new Map<string, string>();
  const inflight = new Map<string, Promise<string>>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  function cancelSettle(documentId: string): void {
    const timer = timers.get(documentId);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.delete(documentId);
    }
  }

  /// Both rejection arms collapse to '': Security (the document denies
  /// CopyContents — normally pre-empted by the registry override in
  /// pdfEngine.ts) and NotFound (the selection was cleared before the fetch
  /// landed). Neither is copyable, and swallowing them here is also what keeps
  /// the eager prefetch from raising an unhandled rejection.
  function fetchText(documentId: string): Promise<string> {
    return selection
      .getSelectedText(documentId)
      .toPromise()
      .then(
        (parts) => (parts ?? []).join('\n'),
        () => '',
      )
      .then((text) => (text.trim() ? text : ''));
  }

  function prefetch(documentId: string): void {
    cancelSettle(documentId);
    const done = fetchText(documentId);
    inflight.set(documentId, done);
    void done.then((text) => {
      // Identity check, not presence: a newer selection on this same document
      // has already replaced the entry, and its result must win however the
      // two round-trips happen to interleave.
      if (inflight.get(documentId) !== done) return;
      inflight.delete(documentId);
      if (text) cache.set(documentId, text);
      else cache.delete(documentId);
    });
  }

  // A PDF selection is starting, so any native selection elsewhere in the app
  // is stale. Dropping it here means `hasDomSelection` in shortcuts.ts can
  // treat "a real DOM selection exists" as an unambiguous signal to stand aside
  // and let the browser copy natively.
  const unsubBegin = selection.onBeginSelection((ev) => {
    if (ev.documentId !== activeDocumentId()) return;
    clearNativeSelection?.();
  });

  // Fires on every glyph the range grows by, and with a null range on the
  // pointerdown that clears it. Either way the cached text is stale as of this
  // instant, so it is dropped first and only re-fetched once the range settles.
  const unsubChange = selection.onSelectionChange((ev) => {
    cache.delete(ev.documentId);
    inflight.delete(ev.documentId);
    cancelSettle(ev.documentId);
    if (!ev.selection || ev.documentId !== activeDocumentId()) return;
    timers.set(
      ev.documentId,
      setTimeout(() => {
        timers.delete(ev.documentId);
        prefetch(ev.documentId);
      }, SETTLE_MS),
    );
  });

  const unsubEnd = selection.onEndSelection((ev) => {
    if (ev.documentId !== activeDocumentId()) return;
    prefetch(ev.documentId);
  });

  function write(text: string): Promise<void> {
    return copy(text).catch(() => {
      onError?.("Couldn't copy");
    });
  }

  return {
    hasSelection(): boolean {
      const id = activeDocumentId();
      if (!id) return false;
      // A pending settle counts: the user has a selection on screen and simply
      // beat the timer to the keystroke. Claiming ⌘C here sends them down
      // copySelection's cold path rather than letting the browser no-op.
      return cache.has(id) || inflight.has(id) || timers.has(id);
    },

    async copySelection(): Promise<void> {
      const id = activeDocumentId();
      if (!id) return;
      const cached = cache.get(id);
      // The whole point of the eager fetch: `write` is reached synchronously,
      // inside the keydown, while the user gesture still authorizes a clipboard
      // write. Never put an await in front of this line.
      if (cached) return write(cached);
      // Cold or still in flight — the user out-raced the prefetch. Reuse the
      // in-flight fetch, or start one through prefetch so it also cancels any
      // pending settle timer (no double round-trip) and warms the cache for
      // the retry. Chrome's transient activation (~5s) survives the round-trip;
      // WebKit's may not, in which case the error toast fires and a second ⌘C,
      // now warm, takes the synchronous path above.
      if (!inflight.has(id)) prefetch(id);
      const text = await inflight.get(id);
      if (text) await write(text);
    },

    forget(documentId: string): void {
      cache.delete(documentId);
      inflight.delete(documentId);
      cancelSettle(documentId);
    },

    destroy(): void {
      unsubBegin();
      unsubChange();
      unsubEnd();
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      cache.clear();
      inflight.clear();
    },
  };
}

// The live instance, published by PdfDeck. shortcuts.ts is a plain module with
// no access to the EmbedPDF context — same division of labour as readerState's
// openFind, which the keymap calls into rather than reaching into the reader.
let current: PdfCopy | null = null;

export function registerPdfCopy(copier: PdfCopy | null): void {
  current = copier;
}

export function pdfSelectionHasText(): boolean {
  return current?.hasSelection() ?? false;
}

export function copyPdfSelection(): void {
  void current?.copySelection();
}

export function forgetPdfSelection(documentId: string): void {
  current?.forget(documentId);
}
