/** Given the docs currently opened and the current tab ids, decide which to
 *  open and which to close. Pure — the caller performs the side effects. */
export function reconcileDocuments(
  opened: Iterable<string>,
  tabIds: string[],
): { toOpen: string[]; toClose: string[] } {
  const openedSet = new Set(opened);
  const tabSet = new Set(tabIds);
  const toOpen = tabIds.filter((id) => !openedSet.has(id));
  const toClose = [...openedSet].filter((id) => !tabSet.has(id));
  return { toOpen, toClose };
}

/** Split the documents that need opening into the one the user is actually
 *  looking at and the rest, which the caller defers.
 *
 *  Opening every restored tab at once is what made a reload crawl. EmbedPDF
 *  runs document opens through a single engine lane (the task queue is
 *  concurrency 1) at CRITICAL priority — above renderPage — and a started task
 *  is never preempted. So each background tab's parse lands *ahead* of the
 *  visible tab's page rasters, and a session restored with four tabs paints
 *  nothing until all four have been parsed and their pages rasterized: the
 *  renderer stayed jammed for tens of seconds. Deferring is not "don't load
 *  background tabs" — they still load, just behind the one on screen, so
 *  switching to them stays instant. */
export function planOpens(
  toOpen: string[],
  activeId: string | null,
): { now: string[]; deferred: string[] } {
  // Nothing is on screen when there's no active tab, so nothing earns priority.
  const now = activeId !== null && toOpen.includes(activeId) ? [activeId] : [];
  const deferred = toOpen.filter((id) => id !== activeId);
  return { now, deferred };
}
