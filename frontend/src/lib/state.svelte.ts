import { deletePaper, getPaper, getStats, importPaper, listPapers } from './api';
import type { Filters, PaperDetail, PaperSummary, Stats } from './types';

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
