import { getPaper, getStats, listPapers } from './api';
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

export const ui = $state<{ sidebarOpen: boolean }>({ sidebarOpen: true });
export function toggleSidebar(): void {
  ui.sidebarOpen = !ui.sidebarOpen;
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
