import {
  deletePaper,
  getPaper,
  getStats,
  identifyPaper,
  identifySearch,
  importPaper,
  listPapers,
} from './api';
import type { Candidate, Filters, IdentifyBody, PaperDetail, PaperSummary, Stats } from './types';

export const filters = $state<Filters>({ q: '', status: 'all', sort: 'year_desc' });

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

export const theme = $state<{ mode: 'light' | 'dark' }>({ mode: 'light' });

export const ui = $state<{ sidebarOpen: boolean; importOpen: boolean }>({
  sidebarOpen: true,
  importOpen: false,
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

function applyTheme(): void {
  document.documentElement.classList.toggle('dark', theme.mode === 'dark');
}
export function initTheme(): void {
  const saved = localStorage.getItem('xuewen-theme');
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  theme.mode = saved === 'dark' || (!saved && prefersDark) ? 'dark' : 'light';
  applyTheme();
}
export function toggleTheme(): void {
  theme.mode = theme.mode === 'dark' ? 'light' : 'dark';
  localStorage.setItem('xuewen-theme', theme.mode);
  applyTheme();
}

export interface ImportItem {
  name: string;
  status: 'queued' | 'importing' | 'ingested' | 'duplicate' | 'same-work' | 'in-trash' | 'failed';
  message?: string;
  needsReview?: boolean;
}

export const importState = $state<{ items: ImportItem[]; cancelled: boolean }>({
  items: [],
  cancelled: false,
});

// Files waiting to upload, paired with their row index in importState.items
// and the import session they belong to.
const pending: { file: File; index: number; session: number }[] = [];
let draining: Promise<void> | null = null;
let importSession = 0;

/// Queue files for import and (re)start the sequential drain. Resolves when the
/// current batch finishes.
export function enqueueFiles(files: File[]): Promise<void> {
  const session = importSession;
  for (const file of files) {
    const index = importState.items.push({ name: file.name, status: 'queued' }) - 1;
    pending.push({ file, index, session });
  }
  if (!draining) {
    draining = drainQueue().finally(() => {
      draining = null;
    });
  }
  return draining;
}

async function drainQueue(): Promise<void> {
  while (pending.length > 0) {
    const job = pending.shift()!;
    // Skip work that was cancelled or belongs to a superseded import session.
    if (importState.cancelled || job.session !== importSession) continue;
    importState.items[job.index].status = 'importing';
    try {
      const res = await importPaper(job.file);
      if (job.session !== importSession) continue; // a new session started mid-upload
      if (res.outcome === 'duplicate') {
        importState.items[job.index].status = 'duplicate';
      } else if (res.outcome === 'same_work') {
        importState.items[job.index].status = 'same-work';
      } else if (res.outcome === 'in_trash') {
        importState.items[job.index].status = 'in-trash';
        importState.items[job.index].message = res.id;
      } else {
        importState.items[job.index].status = 'ingested';
        importState.items[job.index].message = res.title ?? '(untitled)';
        importState.items[job.index].needsReview = res.status === 'needs_review';
      }
    } catch (e) {
      if (job.session !== importSession) continue;
      importState.items[job.index].status = 'failed';
      importState.items[job.index].message = (e as Error).message;
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
