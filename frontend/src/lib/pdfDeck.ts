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
