import { SvelteMap } from 'svelte/reactivity';
import {
  addPaperToProject,
  addTag,
  createProject,
  deletePaper,
  restorePaper,
  deleteProject,
  getPaper,
  getStats,
  listPapers,
  listProjects,
  listTags,
  removePaperFromProject,
  removeTag,
  renameTag as apiRenameTag,
  deleteTag as apiDeleteTag,
  searchPapers,
  setPaperName as apiSetPaperName,
  setStar,
  updateProject,
} from './api';
import { invalidateLibraryTitleIndex } from './citationMatch';
import { hasSearchTerms, parseQuery, setQualifier } from './searchQuery';
import { applyQueryEdit, filters, searchMeta, searchOpts } from './searchState.svelte';
import { closeTab, saveTabs, selection, viewer } from './tabs.svelte';
import { toast } from './toasts.svelte';
import type {
  BibFormat,
  PaperDetail,
  PaperSummary,
  Project,
  Stats,
  TagSummary,
} from './types';

export const library = $state<{
  papers: PaperSummary[];
  loading: boolean;
  error: string | null;
}>({ papers: [], loading: false, error: null });

export const stats = $state<{ value: Stats | null }>({ value: null });

export const projects = $state<{ items: Project[] }>({ items: [] });

export const tags = $state<{ items: TagSummary[] }>({ items: [] });

export const bibFormat = $state<{ value: BibFormat }>({ value: 'bibtex' });

export async function loadStats(): Promise<void> {
  try {
    stats.value = await getStats();
  } catch (e) {
    console.error(e);
  }
}

let seq = 0;
export async function loadPapers(opts?: { keywordOnly?: boolean }): Promise<void> {
  const my = ++seq;
  library.loading = true;
  library.error = null;
  try {
    const parsed = parseQuery(filters.q);
    if (!hasSearchTerms(parsed)) {
      // Qualifier-only (or empty) query → plain filtered list. The parsed
      // filters are already cached on `filters`; q itself must not leak in.
      const papers = await listPapers({ ...filters, q: '' });
      if (my !== seq) return; // a newer request superseded this one
      library.papers = papers;
      searchMeta.byId = {};
    } else {
      const keywordOnly = Boolean(opts?.keywordOnly) || !searchOpts.semantic;
      const resp = await searchPapers(filters.q, { ...searchOpts }, keywordOnly);
      if (my !== seq) return;
      library.papers = resp.results.map((r) => r.paper);
      searchMeta.byId = Object.fromEntries(resp.results.map((r) => [r.paper.id, r.match]));
      searchMeta.semantic = { available: resp.semantic.available, reason: resp.semantic.reason };
    }
  } catch (e) {
    if (my === seq) library.error = (e as Error).message;
  } finally {
    if (my === seq) library.loading = false;
  }
}

export async function loadProjects(): Promise<void> {
  try {
    projects.items = await listProjects();
  } catch (e) {
    console.error(e);
  }
}

export async function loadTags(): Promise<void> {
  try {
    tags.items = await listTags();
  } catch (e) {
    console.error(e);
  }
}

/// Cached full record per paper id. Reactive (SvelteMap), so an open view
/// (DockDetails) re-renders in place when a mutation patches a record and
/// refetches when one is evicted — no remount counter, no double bookkeeping.
/// Reactivity is key-level only: writes must go through `set` with a NEW
/// object (what `patchDetail` does); mutating a stored object in place would
/// go unnoticed.
const details = new SvelteMap<string, PaperDetail>();

export function cachedDetail(id: string): PaperDetail | undefined {
  return details.get(id);
}

/// Merge a partial update into a cached detail. A cache miss is a no-op —
/// the next `loadDetail` fetches fresh anyway.
export function patchDetail(id: string, patch: Partial<PaperDetail>): void {
  const cur = details.get(id);
  if (cur) details.set(id, { ...cur, ...patch });
}

/// Replace a cached record outright (identify's authoritative server echo).
export function setDetail(id: string, d: PaperDetail): void {
  details.set(id, d);
}

export function evictDetail(id: string): void {
  details.delete(id);
}

/// A global tag/project rename or delete stales the names embedded in every
/// cached record: evict everything and let open views refetch on miss.
export function clearDetails(): void {
  details.clear();
}

export async function loadDetail(id: string): Promise<PaperDetail> {
  const cached = details.get(id);
  if (cached) return cached;
  const d = await getPaper(id);
  details.set(id, d);
  return d;
}

export async function createNewProject(name: string): Promise<Project> {
  const p = await createProject(name);
  await loadProjects();
  return p;
}

export async function renameProject(id: string, patch: { name?: string }): Promise<void> {
  const wasActive = filters.project === id;
  await updateProject(id, patch);
  clearDetails();
  await loadProjects();
  // The `project:` qualifier is name-keyed, so an active filter must follow
  // the rename in the search string (the single source of truth) — the old
  // name would stop resolving to an id and silently match nothing on the
  // next sync. loadProjects ran first so the new name resolves.
  if (wasActive && patch.name) await applyQueryEdit(setQualifier(filters.q, 'project', patch.name));
  else await loadPapers();
}

export async function removeProject(id: string): Promise<void> {
  const wasActive = filters.project === id;
  await deleteProject(id);
  clearDetails();
  await loadProjects();
  // Drop the dead qualifier from the search string itself — clearing only the
  // cached filters.project would leave `project:` in the box to resurrect the
  // filter on the next sync.
  if (wasActive) await applyQueryEdit(setQualifier(filters.q, 'project', null));
  else await loadPapers();
}

export async function addToProject(paperId: string, projectId: string): Promise<void> {
  await addPapersToProject([paperId], projectId);
}

/// Bulk variant: the memberships are added in parallel and the project list /
/// paper list refresh happens ONCE — per-paper `addToProject` calls would
/// refetch them once per paper. A partial failure still refreshes for the
/// memberships that landed, then rejects naming the failed count, so the
/// caller's error surface tells the truth about what happened.
export async function addPapersToProject(paperIds: string[], projectId: string): Promise<void> {
  const results = await Promise.allSettled(paperIds.map((id) => addPaperToProject(id, projectId)));
  const added = paperIds.filter((_, i) => results[i].status === 'fulfilled');
  for (const id of added) evictDetail(id);
  if (added.length > 0) {
    await loadProjects();
    if (filters.project === projectId) await loadPapers();
  }
  const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
  if (failures.length > 0) {
    if (paperIds.length === 1) throw failures[0].reason;
    throw new Error(
      `couldn't add ${failures.length} of ${paperIds.length} papers: ${(failures[0].reason as Error).message}`,
    );
  }
}

export async function removeFromProject(paperId: string, projectId: string): Promise<void> {
  await removePaperFromProject(paperId, projectId);
  evictDetail(paperId);
  await loadProjects();
  if (filters.project === projectId) await loadPapers();
}

/// Flip a paper's starred flag optimistically: patch the row/cached detail
/// first so the star moves instantly, then call the API and roll back (with
/// an error toast) if it rejects. When the starred filter is active the list
/// itself may need to drop/gain the paper, so it reloads after the call.
export async function toggleStar(paperId: string): Promise<void> {
  const row = library.papers.find((p) => p.id === paperId);
  const prev = row?.starred ?? cachedDetail(paperId)?.starred ?? false;
  const next = !prev;
  if (row) row.starred = next;
  patchDetail(paperId, { starred: next });
  try {
    await setStar(paperId, next);
  } catch (e) {
    if (row) row.starred = prev;
    patchDetail(paperId, { starred: prev });
    toast('error', `Couldn't update star: ${(e as Error).message}`);
    return;
  }
  if (filters.starred !== undefined) await loadPapers();
}

/// Set (or clear) a paper's manual name. Await first, then patch the row/
/// cached detail from the server's echo (authoritative: it re-trims and
/// normalizes empty to null). Name is never a filter, so the list only
/// reloads when its order depends on it.
export async function setPaperName(paperId: string, name: string | null): Promise<void> {
  const { name: stored } = await apiSetPaperName(paperId, name);
  const row = library.papers.find((p) => p.id === paperId);
  if (row) row.name = stored;
  patchDetail(paperId, { name: stored });
  // The strip labels an open tab by its name, so a rename has to reach the tab
  // too — otherwise the label stays stale until the next reload.
  const tab = viewer.tabs.find((t) => t.id === paperId);
  if (tab) {
    tab.name = stored;
    saveTabs();
  }
  if (filters.sort === 'name') await loadPapers();
}

/// Add a tag (by name; creating it if new) to a paper, patch the row/cached
/// detail, and refresh the tags store (name list + counts).
export async function addTagToPaper(paperId: string, name: string): Promise<void> {
  await addTagToPapers([paperId], name);
}

/// Bulk variant: rows are patched per paper but the tags store / paper list
/// refresh happens ONCE at the end — per-paper `addTagToPaper` calls would
/// refetch them once per paper. The adds stay sequential (not Promise.all):
/// a new tag name is created by whichever add lands first, and racing that
/// creation across requests is not worth the latency win.
export async function addTagToPapers(paperIds: string[], name: string): Promise<void> {
  for (const paperId of paperIds) {
    const tag = await addTag(paperId, name);
    const row = library.papers.find((p) => p.id === paperId);
    if (row && !row.tags.some((t) => t.id === tag.id)) row.tags = [...row.tags, tag];
    const cached = cachedDetail(paperId);
    if (cached && !cached.tags.some((t) => t.id === tag.id))
      patchDetail(paperId, { tags: [...cached.tags, tag] });
  }
  await loadTags();
  if (filters.tag) await loadPapers();
}

export async function removeTagFromPaper(paperId: string, tagId: string): Promise<void> {
  await removeTag(paperId, tagId);
  const row = library.papers.find((p) => p.id === paperId);
  if (row) row.tags = row.tags.filter((t) => t.id !== tagId);
  const cached = cachedDetail(paperId);
  if (cached) patchDetail(paperId, { tags: cached.tags.filter((t) => t.id !== tagId) });
  await loadTags();
  if (filters.tag) await loadPapers();
}

/// Rename a tag globally (not per-paper): refresh the tags store and reload
/// the paper list so row chips pick up the new name. The `tag:` qualifier is
/// name-keyed, so if the renamed tag was the active filter the search string
/// is rewritten and the filter follows the rename — to the server's echo, not
/// the typed name: rename normalizes ('nlp / eval' is stored as 'nlp/eval'),
/// and the tag filter is an exact name match, so the raw string would leave a
/// qualifier that matches nothing.
export async function renameTag(id: string, name: string): Promise<void> {
  const tag = tags.items.find((t) => t.id === id);
  const stored = await apiRenameTag(id, name);
  clearDetails();
  await loadTags();
  if (tag && filters.tag === tag.name)
    await applyQueryEdit(setQualifier(filters.q, 'tag', stored.name));
  else await loadPapers();
}

/// Delete a tag from every paper carrying it (GC'd tag row included), then
/// refresh the tags store and paper list. If it was the active filter, the
/// dead qualifier is dropped from the search string itself — clearing only
/// the cached filters.tag would leave `tag:` in the box to resurrect an
/// always-empty filter on the next sync.
export async function deleteTag(id: string): Promise<void> {
  const tag = tags.items.find((t) => t.id === id);
  await apiDeleteTag(id);
  clearDetails();
  await loadTags();
  if (tag && filters.tag === tag.name) await applyQueryEdit(setQualifier(filters.q, 'tag', null));
  else await loadPapers();
}

/// Soft-delete one paper on the server and drop it from the UI: close its
/// tab, remove it from the list, forget its cached detail.
async function dropPaper(id: string): Promise<void> {
  await deletePaper(id);
  closeTab(id);
  library.papers = library.papers.filter((p) => p.id !== id);
  evictDetail(id);
  invalidateLibraryTitleIndex();
  if (selection.id === id) selection.id = null;
}

/// Delete a paper with an Undo toast (deletes are soft — POST /restore
/// un-trashes) on a longer timeout so there's time to reach for it.
export async function removePaper(id: string): Promise<void> {
  await removePapers([id]);
}

/// Bulk delete: every id is dropped, then ONE combined toast carries the
/// Undo for the whole batch — per-paper toasts would stack unusably.
/// Failures surface here too, as one combined error toast, so callers need
/// no catch of their own (this never rejects); the Undo restores only the
/// ids that were actually deleted.
export async function removePapers(ids: string[]): Promise<void> {
  const results = await Promise.allSettled(ids.map((id) => dropPaper(id)));
  await loadStats();
  const deleted = ids.filter((_, i) => results[i].status === 'fulfilled');
  const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
  if (deleted.length > 0) {
    const message = deleted.length === 1 ? 'Paper deleted' : `${deleted.length} papers deleted`;
    toast('success', message, 8000, { label: 'Undo', run: () => void undoDelete(deleted) });
  }
  if (failures.length > 0) {
    const label =
      failures.length === 1 ? "Couldn't delete" : `Couldn't delete ${failures.length} papers`;
    toast('error', `${label}: ${(failures[0].reason as Error).message}`);
  }
}

async function undoDelete(ids: string[]): Promise<void> {
  try {
    await Promise.all(ids.map((id) => restorePaper(id)));
    invalidateLibraryTitleIndex();
    await loadPapers();
    await loadStats();
    toast('success', ids.length === 1 ? 'Paper restored' : `${ids.length} papers restored`);
  } catch (e) {
    toast('error', `Couldn't restore: ${(e as Error).message}`);
  }
}
