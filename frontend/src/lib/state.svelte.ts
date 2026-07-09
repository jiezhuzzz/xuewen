import {
  addPaperToProject,
  createProject,
  deletePaper,
  deleteProject,
  exportPaper,
  getPaper,
  getStats,
  identifyPaper,
  identifySearch,
  importPaper,
  importUrl,
  listPapers,
  listProjects,
  removePaperFromProject,
  updateProject,
} from './api';
import type {
  BibFormat,
  Candidate,
  Filters,
  IdentifyBody,
  PaperDetail,
  PaperSummary,
  Project,
  Stats,
} from './types';

export const filters = $state<Filters>({ q: '', status: 'all', sort: 'year_desc', project: 'all' });

export const projects = $state<{ items: Project[] }>({ items: [] });

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

/// Fetch a paper's citation in the current format and copy it to the clipboard.
export async function copyCitation(id: string): Promise<void> {
  const text = await exportPaper(id, bibFormat.value);
  await copyText(text);
}

export const library = $state<{
  papers: PaperSummary[];
  loading: boolean;
  error: string | null;
}>({ papers: [], loading: false, error: null });

export const stats = $state<{ value: Stats | null }>({ value: null });

export interface Tab {
  id: string;
  title: string;
}
export const viewer = $state<{ tabs: Tab[]; activeId: string | null; infoOpen: boolean }>({
  tabs: [],
  activeId: null,
  infoOpen: false,
});

export type ThemeMode = 'light' | 'dark' | 'system';
export const theme = $state<{ mode: ThemeMode }>({ mode: 'system' });

export const ui = $state<{ sidebarOpen: boolean; importOpen: boolean; projectsOpen: boolean }>({
  sidebarOpen: true,
  importOpen: false,
  projectsOpen: false,
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
export function openProjects(): void {
  ui.projectsOpen = true;
}
export function closeProjects(): void {
  ui.projectsOpen = false;
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
export async function loadPapers(): Promise<void> {
  const my = ++seq;
  library.loading = true;
  library.error = null;
  try {
    const papers = await listPapers({ ...filters });
    if (my !== seq) return; // a newer request superseded this one
    library.papers = papers;
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

export async function setProjectFilter(id: string): Promise<void> {
  filters.project = id;
  await loadPapers();
}

export async function createNewProject(name: string, note: string | null): Promise<Project> {
  const p = await createProject(name, note);
  await loadProjects();
  return p;
}

export async function renameProject(
  id: string,
  patch: { name?: string; note?: string | null },
): Promise<void> {
  await updateProject(id, patch);
  await loadProjects();
}

export async function removeProject(id: string): Promise<void> {
  await deleteProject(id);
  if (filters.project === id) filters.project = 'all';
  await loadProjects();
  await loadPapers();
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

let debounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  clearTimeout(debounce);
  debounce = setTimeout(loadPapers, 200);
}

export function openTab(p: PaperSummary): void {
  if (!viewer.tabs.some((t) => t.id === p.id)) {
    viewer.tabs.push({ id: p.id, title: p.title ?? p.cite_key ?? p.id });
  }
  viewer.activeId = p.id;
}

export function closeTab(id: string): void {
  const idx = viewer.tabs.findIndex((t) => t.id === id);
  if (idx === -1) return;
  viewer.tabs.splice(idx, 1);
  if (viewer.activeId === id) {
    viewer.activeId = viewer.tabs[Math.max(0, idx - 1)]?.id ?? null;
  }
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
  applyTheme();
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
/// mounted views (InfoPanel) re-run loadDetail and pick up the fresh record.
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
