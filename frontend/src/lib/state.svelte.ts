import {
  addPaperToProject,
  addTag,
  createProject,
  deletePaper,
  deleteProject,
  exportPaper,
  getPaper,
  getSearchStatus,
  getSettings,
  getStats,
  identifyPaper,
  identifySearch,
  importPaper,
  importUrl,
  listPapers,
  listProjects,
  listTags,
  removePaperFromProject,
  removeTag,
  renameTag as apiRenameTag,
  deleteTag as apiDeleteTag,
  searchPapers,
  setStar,
  updateProject,
} from './api';
import { invalidateLibraryTitleIndex } from './citationMatch';
import { dur } from './motion';
import { dropReaderState } from './readerState.svelte';
import { toast } from './toasts.svelte';
import type {
  BibFormat,
  Candidate,
  Filters,
  IdentifyBody,
  PaperDetail,
  PaperSummary,
  Project,
  SearchMatch,
  SearchOpts,
  Stats,
  TagSummary,
  TranslateSettings,
} from './types';

export const filters = $state<Filters>({
  q: '',
  status: 'all',
  sort: 'year_desc',
  project: 'all',
  tag: undefined,
  starred: undefined,
});

export const searchOpts = $state<SearchOpts>({
  title: true,
  authors: true,
  abstract: true,
  body: true,
  keyword: true,
  semantic: true,
});

/// Match info per paper id for the current search, plus the semantic tier's
/// availability (from the last response or /api/search/status).
export const searchMeta = $state<{
  byId: Record<string, SearchMatch>;
  semantic: { available: boolean; reason: string | null };
  /// Papers still waiting for a tier to index (drives "indexing N papers…").
  pending: number;
}>({ byId: {}, semantic: { available: true, reason: null }, pending: 0 });

/// Semantic chip is disabled when the backend can't serve it or the field
/// selection makes it meaningless (authors-only).
export function semanticBlocked(): boolean {
  const authorsOnly =
    searchOpts.authors && !searchOpts.title && !searchOpts.abstract && !searchOpts.body;
  return authorsOnly || !searchMeta.semantic.available;
}

export function toggleSearchField(k: 'title' | 'authors' | 'abstract' | 'body'): void {
  const on = ['title', 'authors', 'abstract', 'body'].filter(
    (f) => searchOpts[f as keyof SearchOpts],
  );
  if (searchOpts[k] && on.length === 1) return; // keep at least one field
  searchOpts[k] = !searchOpts[k];
  if (filters.q.trim()) void loadPapers();
}

export function toggleSearchEngine(k: 'keyword' | 'semantic'): void {
  const other = k === 'keyword' ? 'semantic' : 'keyword';
  if (searchOpts[k] && !searchOpts[other]) return; // keep at least one engine
  searchOpts[k] = !searchOpts[k];
  if (filters.q.trim()) void loadPapers();
}

export async function loadSearchStatus(): Promise<void> {
  try {
    const st = await getSearchStatus();
    searchMeta.semantic = { available: st.semantic_available, reason: st.reason };
    searchMeta.pending = Math.max(st.fts.pending, st.vectors.pending);
  } catch (e) {
    console.error(e); // e.g. 503 search not configured -> leave defaults
  }
}

export const projects = $state<{ items: Project[] }>({ items: [] });

export const tags = $state<{ items: TagSummary[] }>({ items: [] });

export const bibFormat = $state<{ value: BibFormat }>({ value: 'bibtex' });

/// Copy text to the clipboard. Uses the async Clipboard API when available
/// (secure contexts: https or localhost), and otherwise falls back to the
/// legacy execCommand path — which is what makes copy work when the UI is served
/// over plain HTTP to a non-localhost host, where `navigator.clipboard` is
/// undefined. Throws if neither path succeeds.
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.setAttribute('readonly', '');
  ta.style.position = 'fixed';
  ta.style.top = '0';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    if (!document.execCommand('copy')) {
      throw new Error('copy command was rejected');
    }
  } finally {
    document.body.removeChild(ta);
  }
}

/// Fetch a paper's citation and copy it to the clipboard. Defaults to the
/// current format setting; callers with a fixed-label button (e.g. the
/// context menu's "Copy BibTeX") pass an explicit format instead.
export async function copyCitation(id: string, format: BibFormat = bibFormat.value): Promise<void> {
  const text = await exportPaper(id, format);
  await copyText(text);
}

export const library = $state<{
  papers: PaperSummary[];
  loading: boolean;
  error: string | null;
}>({ papers: [], loading: false, error: null });

export const stats = $state<{ value: Stats | null }>({ value: null });

/// UI preferences from the server (`/api/settings`). Loaded once at startup.
export const appSettings = $state<{ foldAbstract: boolean; translate: TranslateSettings }>({
  foldAbstract: true,
  translate: { enabled: false },
});

export async function loadSettings(): Promise<void> {
  try {
    const s = await getSettings();
    appSettings.foldAbstract = s.fold_abstract;
    appSettings.translate = s.translate ?? { enabled: false };
  } catch (e) {
    console.error(e); // keep the default on failure
  }
}

export interface Tab {
  id: string;
  title: string;
}
/// The content pane's tab strip. `activeId === null` means the permanent
/// "Library" home tab is active (shows the Welcome panel); a string means
/// that PDF tab is active. Tabs persist while home is active.
export const viewer = $state<{ tabs: Tab[]; activeId: string | null }>({
  tabs: [],
  activeId: null,
});

/// The browsing highlight for the Library list (moved by j/k). Distinct from viewer.activeId: the highlight is the list cursor; opening a paper reads it.
export const selection = $state<{ id: string | null }>({ id: null });

export function selectPaper(id: string | null): void {
  selection.id = id;
}

export type DockTab = 'details' | 'ask';

/// The reader dock: one right-docked panel hosting the Details and Ask tabs
/// (replaces the old separate info panel + chat float). Open state and tab
/// are remembered across sessions.
export const dock = $state<{ open: boolean; tab: DockTab }>({ open: false, tab: 'details' });

const DOCK_KEY = 'xuewen-dock';

/// Load the remembered dock state (default: closed, Details). Call once at startup.
export function initDock(): void {
  try {
    const raw = localStorage.getItem(DOCK_KEY);
    if (!raw) return;
    const v = JSON.parse(raw) as { open?: unknown; tab?: unknown };
    dock.open = v.open === true;
    dock.tab = v.tab === 'ask' ? 'ask' : 'details';
  } catch {
    /* corrupted value — keep defaults */
  }
}

function saveDock(): void {
  try {
    localStorage.setItem(DOCK_KEY, JSON.stringify({ open: dock.open, tab: dock.tab }));
  } catch {
    /* no localStorage — state still applies, only persistence is lost */
  }
}

export function openDock(tab: DockTab): void {
  dock.open = true;
  dock.tab = tab;
  saveDock();
}

export function closeDock(): void {
  dock.open = false;
  saveDock();
}

/// The `i`/`c` shortcut behavior: close if already open on that tab,
/// otherwise open on (or switch to) it. The dock only exists over a PDF.
export function toggleDock(tab: DockTab): void {
  if (viewer.activeId === null) return;
  if (dock.open && dock.tab === tab) closeDock();
  else openDock(tab);
}

/// Activate the Library home tab (keeps PDF tabs open). Leaving the reader
/// always leaves zen too — zen without a PDF is a blank screen.
export function goHome(): void {
  viewer.activeId = null;
  ui.zen = false;
}

/// Zen requires an active PDF tab; toggling from home is a no-op.
export function toggleZen(): void {
  ui.zen = viewer.activeId !== null && !ui.zen;
}

export type ThemeMode = 'light' | 'dark' | 'system';
export const theme = $state<{ mode: ThemeMode }>({ mode: 'system' });

export const ui = $state<{
  sidebarOpen: boolean;
  importOpen: boolean;
  zen: boolean;
  paletteOpen: boolean;
}>({
  sidebarOpen: true,
  importOpen: false,
  zen: false,
  paletteOpen: false,
});
export function toggleSidebar(): void {
  ui.sidebarOpen = !ui.sidebarOpen;
}
export function openImport(): void {
  importSession++;
  pending.length = 0;
  importState.items = [];
  importState.cancelled = false;
  ui.importOpen = true;
}
export function closeImport(): void {
  importState.cancelled = true;
  ui.importOpen = false;
}

const detailCache = new Map<string, PaperDetail>();

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
    const q = filters.q.trim();
    if (!q) {
      const papers = await listPapers({ ...filters });
      if (my !== seq) return; // a newer request superseded this one
      library.papers = papers;
      searchMeta.byId = {};
    } else {
      const keywordOnly = Boolean(opts?.keywordOnly) || !searchOpts.semantic;
      const resp = await searchPapers(q, { ...searchOpts }, { ...filters }, keywordOnly);
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

/// The three list filters (project/tag/starred) are mutually exclusive in the
/// UI: setting one clears the other two so exactly one is ever active.
export async function setProjectFilter(id: string): Promise<void> {
  filters.project = id;
  filters.tag = undefined;
  filters.starred = undefined;
  await loadPapers();
}

export async function setTagFilter(tag: string | undefined): Promise<void> {
  filters.tag = tag;
  filters.project = 'all';
  filters.starred = undefined;
  await loadPapers();
}

export async function setStarFilter(on: boolean): Promise<void> {
  filters.starred = on || undefined;
  filters.project = 'all';
  filters.tag = undefined;
  await loadPapers();
}

/// Whether any list filter deviates from the default view — i.e. whether an
/// empty list means "nothing matches" rather than "the library is empty".
export function anyFilterActive(): boolean {
  return (
    filters.q.trim() !== '' ||
    filters.status !== 'all' ||
    filters.project !== 'all' ||
    filters.tag !== undefined ||
    filters.starred !== undefined
  );
}

/// Reset every list filter (search, status, project/tag/star) to the default
/// view and reload — the escape hatch offered by the list's empty state.
export async function clearFilters(): Promise<void> {
  filters.q = '';
  filters.status = 'all';
  filters.project = 'all';
  filters.tag = undefined;
  filters.starred = undefined;
  await loadPapers();
}

export async function createNewProject(name: string): Promise<Project> {
  const p = await createProject(name);
  await loadProjects();
  return p;
}

export async function renameProject(id: string, patch: { name?: string }): Promise<void> {
  await updateProject(id, patch);
  await loadProjects();
  await loadPapers();
  detailCache.clear();
  detailRefresh.n += 1;
}

export async function removeProject(id: string): Promise<void> {
  await deleteProject(id);
  if (filters.project === id) filters.project = 'all';
  await loadProjects();
  await loadPapers();
  detailCache.clear();
  detailRefresh.n += 1;
}

export async function addToProject(paperId: string, projectId: string): Promise<void> {
  await addPaperToProject(paperId, projectId);
  detailCache.delete(paperId);
  detailRefresh.n += 1;
  await loadProjects();
  if (filters.project === projectId) await loadPapers();
}

export async function removeFromProject(paperId: string, projectId: string): Promise<void> {
  await removePaperFromProject(paperId, projectId);
  detailCache.delete(paperId);
  detailRefresh.n += 1;
  await loadProjects();
  if (filters.project === projectId) await loadPapers();
}

/// Flip a paper's starred flag optimistically: patch the row/cached detail
/// first so the star moves instantly, then call the API and roll back (with
/// an error toast) if it rejects. When the starred filter is active the list
/// itself may need to drop/gain the paper, so it reloads after the call.
export async function toggleStar(paperId: string): Promise<void> {
  const row = library.papers.find((p) => p.id === paperId);
  const cached = detailCache.get(paperId);
  const prev = row?.starred ?? cached?.starred ?? false;
  const next = !prev;
  if (row) row.starred = next;
  if (cached) cached.starred = next;
  detailRefresh.n += 1;
  try {
    await setStar(paperId, next);
  } catch (e) {
    if (row) row.starred = prev;
    if (cached) cached.starred = prev;
    detailRefresh.n += 1;
    toast('error', `Couldn't update star: ${(e as Error).message}`);
    return;
  }
  if (filters.starred !== undefined) await loadPapers();
}

/// Add a tag (by name; creating it if new) to a paper, patch the row/cached
/// detail, and refresh the tags store (name list + counts).
export async function addTagToPaper(paperId: string, name: string): Promise<void> {
  const tag = await addTag(paperId, name);
  const row = library.papers.find((p) => p.id === paperId);
  if (row && !row.tags.some((t) => t.id === tag.id)) row.tags = [...row.tags, tag];
  const cached = detailCache.get(paperId);
  if (cached && !cached.tags.some((t) => t.id === tag.id)) cached.tags = [...cached.tags, tag];
  detailRefresh.n += 1;
  await loadTags();
  if (filters.tag) await loadPapers();
}

export async function removeTagFromPaper(paperId: string, tagId: string): Promise<void> {
  await removeTag(paperId, tagId);
  const row = library.papers.find((p) => p.id === paperId);
  if (row) row.tags = row.tags.filter((t) => t.id !== tagId);
  const cached = detailCache.get(paperId);
  if (cached) cached.tags = cached.tags.filter((t) => t.id !== tagId);
  detailRefresh.n += 1;
  await loadTags();
  if (filters.tag) await loadPapers();
}

/// Rename a tag globally (not per-paper): refresh the tags store and reload
/// the paper list so row chips pick up the new name. If the renamed tag was
/// the active filter, clear it (the filter is name-keyed, so it would no
/// longer match under the old name).
export async function renameTag(id: string, name: string): Promise<void> {
  const tag = tags.items.find((t) => t.id === id);
  await apiRenameTag(id, name);
  if (tag && filters.tag === tag.name) filters.tag = undefined;
  await loadTags();
  await loadPapers();
  detailCache.clear();
  detailRefresh.n += 1;
}

/// Delete a tag from every paper carrying it (GC'd tag row included), then
/// refresh the tags store and paper list, clearing the tag filter if it was
/// the one deleted.
export async function deleteTag(id: string): Promise<void> {
  const tag = tags.items.find((t) => t.id === id);
  await apiDeleteTag(id);
  if (tag && filters.tag === tag.name) filters.tag = undefined;
  await loadTags();
  await loadPapers();
  detailCache.clear();
  detailRefresh.n += 1;
}

let kwDebounce: ReturnType<typeof setTimeout> | undefined;
let fullDebounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  if (!q.trim()) {
    void loadPapers();
    return;
  }
  // Fast keyword-only pass while typing; the full (semantic) pass once settled.
  if (searchOpts.keyword) {
    kwDebounce = setTimeout(() => void loadPapers({ keywordOnly: true }), 150);
  }
  if (searchOpts.semantic && !semanticBlocked()) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  } else if (!searchOpts.keyword) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  }
}

export function openTab(p: PaperSummary): void {
  if (!viewer.tabs.some((t) => t.id === p.id)) {
    viewer.tabs.push({ id: p.id, title: p.title ?? p.cite_key ?? p.id });
  }
  viewer.activeId = p.id;
  selection.id = p.id;
}

export function closeTab(id: string): void {
  const idx = viewer.tabs.findIndex((t) => t.id === id);
  if (idx === -1) return;
  viewer.tabs.splice(idx, 1);
  dropReaderState(id);
  if (viewer.activeId === id) {
    viewer.activeId = viewer.tabs[Math.max(0, idx - 1)]?.id ?? null;
  }
  if (viewer.tabs.length === 0) ui.zen = false;
}

export async function loadDetail(id: string): Promise<PaperDetail> {
  const cached = detailCache.get(id);
  if (cached) return cached;
  const d = await getPaper(id);
  detailCache.set(id, d);
  return d;
}

/// Soft-delete a paper on the server, then drop it from the UI: close its tab,
/// remove it from the list, and refresh the counts.
export async function removePaper(id: string): Promise<void> {
  await deletePaper(id);
  closeTab(id);
  library.papers = library.papers.filter((p) => p.id !== id);
  detailCache.delete(id);
  invalidateLibraryTitleIndex();
  if (selection.id === id) selection.id = null;
  await loadStats();
}

const darkQuery = (): MediaQueryList => window.matchMedia('(prefers-color-scheme: dark)');

// Resolve a mode to whether the dark class should be applied. 'system' tracks
// the live OS preference; explicit modes ignore it.
function resolvesDark(mode: ThemeMode): boolean {
  return mode === 'dark' || (mode === 'system' && darkQuery().matches);
}
function applyTheme(): void {
  document.documentElement.classList.toggle('dark', resolvesDark(theme.mode));
}
export function initTheme(): void {
  const saved = localStorage.getItem('xuewen-theme');
  theme.mode = saved === 'light' || saved === 'dark' || saved === 'system' ? saved : 'system';
  applyTheme();
  // Keep 'system' in sync when the OS preference changes at runtime.
  darkQuery().addEventListener('change', () => {
    if (theme.mode === 'system') applyTheme();
  });
}
const THEME_CYCLE: ThemeMode[] = ['light', 'dark', 'system'];
export function toggleTheme(): void {
  theme.mode = THEME_CYCLE[(THEME_CYCLE.indexOf(theme.mode) + 1) % THEME_CYCLE.length];
  localStorage.setItem('xuewen-theme', theme.mode);
  // Crossfade the whole page where the View Transitions API exists; fall
  // back to an instant swap (also under reduced motion / tests via dur).
  const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
  if (doc.startViewTransition && dur(1) > 0) {
    doc.startViewTransition(() => applyTheme());
  } else {
    applyTheme();
  }
}

export interface ImportItem {
  name: string;
  status:
    | 'queued'
    | 'importing'
    | 'ingested'
    | 'duplicate'
    | 'same-work'
    | 'in-trash'
    | 'unfetched'
    | 'failed';
  message?: string;
  needsReview?: boolean;
}

export const importState = $state<{ items: ImportItem[]; cancelled: boolean }>({
  items: [],
  cancelled: false,
});

// Work waiting to be imported (an uploaded file or a URL/identifier string),
// paired with its row index in importState.items and the import session it
// belongs to.
type Job = { kind: 'file'; file: File } | { kind: 'url'; input: string };
const pending: { job: Job; index: number; session: number }[] = [];
let draining: Promise<void> | null = null;
let importSession = 0;

/// Queue files for import and (re)start the sequential drain. Resolves when the
/// current batch finishes.
export function enqueueFiles(files: File[]): Promise<void> {
  const session = importSession;
  for (const file of files) {
    const index = importState.items.push({ name: file.name, status: 'queued' }) - 1;
    pending.push({ job: { kind: 'file', file }, index, session });
  }
  return startDrain();
}

/// Queue a URL/identifier for import and (re)start the sequential drain.
/// Resolves when the current batch finishes.
export function enqueueUrl(input: string): Promise<void> {
  const session = importSession;
  const index = importState.items.push({ name: input, status: 'queued' }) - 1;
  pending.push({ job: { kind: 'url', input }, index, session });
  return startDrain();
}

function startDrain(): Promise<void> {
  if (!draining) {
    draining = drainQueue().finally(() => {
      draining = null;
    });
  }
  return draining;
}

async function drainQueue(): Promise<void> {
  while (pending.length > 0) {
    const item = pending.shift()!;
    // Skip work that was cancelled or belongs to a superseded import session.
    if (importState.cancelled || item.session !== importSession) continue;
    importState.items[item.index].status = 'importing';
    try {
      const res =
        item.job.kind === 'file' ? await importPaper(item.job.file) : await importUrl(item.job.input);
      if (item.session !== importSession) continue; // a new session started mid-fetch
      if (res.outcome === 'duplicate') {
        importState.items[item.index].status = 'duplicate';
      } else if (res.outcome === 'same_work') {
        importState.items[item.index].status = 'same-work';
      } else if (res.outcome === 'in_trash') {
        importState.items[item.index].status = 'in-trash';
        importState.items[item.index].message = res.id;
      } else if (res.outcome === 'unfetched') {
        importState.items[item.index].status = 'unfetched';
        importState.items[item.index].message = res.title ?? '(untitled)';
      } else {
        importState.items[item.index].status = 'ingested';
        importState.items[item.index].message = res.title ?? '(untitled)';
        importState.items[item.index].needsReview = res.status === 'needs_review';
      }
    } catch (e) {
      if (item.session !== importSession) continue;
      importState.items[item.index].status = 'failed';
      importState.items[item.index].message = (e as Error).message;
    }
  }
  // Reflect the newly ingested papers in the sidebar list and counts.
  invalidateLibraryTitleIndex();
  await loadPapers();
  await loadStats();
}

export const identifyState = $state<{
  open: boolean;
  paperId: string | null;
  input: string;
  busy: boolean;
  candidates: Candidate[];
  selected: Candidate | null;
  /// A direct DOI/arXiv body captured at search time (single fetched record flow).
  direct: IdentifyBody | null;
  /// The paper's identifiers at the time the modal opened, so the modal can
  /// warn when the selected candidate would drop one of them.
  current: { doi: string | null; arxiv_id: string | null } | null;
  error: string | null;
}>({
  open: false,
  paperId: null,
  input: '',
  busy: false,
  candidates: [],
  selected: null,
  direct: null,
  current: null,
  error: null,
});

/// Bumped whenever a paper's cached detail is replaced in place, so already
/// mounted views (DockDetails) re-run loadDetail and pick up the fresh record.
export const detailRefresh = $state({ n: 0 });

// Superseded-session guard (same pattern as importSession): an in-flight
// search/apply from a closed or reopened modal must not write into the
// current session's identifyState.
let identifySession = 0;

function resetIdentifyFields(): void {
  identifyState.input = '';
  identifyState.busy = false;
  identifyState.candidates = [];
  identifyState.selected = null;
  identifyState.direct = null;
  identifyState.current = null;
  identifyState.error = null;
}

export function openIdentify(
  paperId: string,
  current?: { doi: string | null; arxiv_id: string | null },
): void {
  identifySession++;
  resetIdentifyFields();
  identifyState.open = true;
  identifyState.paperId = paperId;
  identifyState.current = current ?? null;
}

export function closeIdentify(): void {
  identifySession++;
  resetIdentifyFields();
  identifyState.open = false;
  identifyState.paperId = null;
}

/// Whether applying the currently selected candidate would drop an
/// identifier (DOI/arXiv id) the paper currently has.
export function dropsIdentifier(s: {
  selected: Candidate | null;
  current: { doi: string | null; arxiv_id: string | null } | null;
}): boolean {
  if (!s.selected || !s.current) return false;
  return Boolean(
    (s.current.doi && !s.selected.doi) || (s.current.arxiv_id && !s.selected.arxiv_id),
  );
}

const DOI_RE = /10\.\d{4,9}\/\S+/;
const ARXIV_RE = /^\d{4}\.\d{4,5}(v\d+)?$/;
const ARXIV_URL_RE = /arxiv\.org\/(?:abs|pdf)\/(\d{4}\.\d{4,5}(?:v\d+)?)/i;

/// Warning for pseudo-DOIs that can never resolve. ACM Digital Library uses
/// the reserved 10.5555 prefix for papers it hosts WITHOUT a registered DOI
/// (typically USENIX/NDSS) — Crossref and doi.org have never heard of them.
export function pseudoDoiHint(direct: IdentifyBody | null): string | null {
  if (direct && 'doi' in direct && direct.doi.startsWith('10.5555/')) {
    return '10.5555/… is an ACM DL internal id, not a registered DOI — it will not resolve; try a title search instead.';
  }
  return null;
}

/// Classify what the user pasted: a DOI (even inside a doi.org URL), an arXiv
/// id (bare or inside an arxiv.org URL), or a title query.
export function classifyIdentifyInput(
  input: string,
): { kind: 'doi' | 'arxiv' | 'title'; value: string } {
  const t = input.trim();
  const doi = t.match(DOI_RE);
  // Strip punctuation that rides along when a DOI is copied out of prose.
  if (doi) return { kind: 'doi', value: doi[0].replace(/[.,;)\]}"']+$/, '') };
  const arxivUrl = t.match(ARXIV_URL_RE);
  if (arxivUrl) return { kind: 'arxiv', value: arxivUrl[1] };
  if (ARXIV_RE.test(t)) return { kind: 'arxiv', value: t };
  return { kind: 'title', value: t };
}

/// Search: title inputs hit /api/identify/search; DOI/arXiv inputs stage a
/// direct apply body (the backend fetches the authoritative record on apply).
export async function runIdentifySearch(): Promise<void> {
  const session = identifySession;
  const { kind, value } = classifyIdentifyInput(identifyState.input);
  identifyState.candidates = [];
  identifyState.selected = null;
  identifyState.direct = null;
  identifyState.error = null;
  if (!value) return;
  identifyState.busy = true;
  try {
    if (kind === 'title') {
      const cands = await identifySearch(value);
      if (session !== identifySession) return; // modal closed/reopened mid-flight
      identifyState.candidates = cands;
      if (!cands.length) identifyState.error = 'no candidates found';
    } else {
      identifyState.direct = kind === 'doi' ? { doi: value } : { arxiv_id: value };
    }
  } catch (e) {
    if (session !== identifySession) return;
    identifyState.error = (e as Error).message;
  } finally {
    if (session === identifySession) identifyState.busy = false;
  }
}

/// Apply the selected candidate (or the staged direct identifier).
export async function applyIdentify(): Promise<void> {
  const session = identifySession;
  const id = identifyState.paperId;
  if (!id) return;
  const body: IdentifyBody | null = identifyState.selected
    ? { candidate: identifyState.selected }
    : identifyState.direct;
  if (!body) return;
  identifyState.busy = true;
  identifyState.error = null;
  try {
    const detail = await identifyPaper(id, body);
    // The server applied the match: refresh caches and lists regardless of
    // whether this identify session is still the live one...
    detailCache.set(id, detail);
    detailRefresh.n += 1;
    invalidateLibraryTitleIndex(); // identify can change the paper's title
    const tab = viewer.tabs.find((t) => t.id === id);
    if (tab) tab.title = detail.title ?? tab.title;
    if (session === identifySession) {
      // ...but only the live session may close the modal.
      identifyState.open = false;
      identifyState.paperId = null;
    }
    await loadPapers();
    await loadStats();
  } catch (e) {
    if (session !== identifySession) return;
    identifyState.error = (e as Error).message;
  } finally {
    if (session === identifySession) identifyState.busy = false;
  }
}
