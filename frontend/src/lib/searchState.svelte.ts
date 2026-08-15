import { getSearchStatus } from './api';
import { loadPapers, projects } from './library.svelte';
import {
  type FieldKey,
  type ParsedQuery,
  FIELD_KEYS,
  SEMANTIC_FIELD_KEYS,
  hasSearchTerms,
  setFieldQualifiers,
  setQualifier,
  setStarredQualifier,
  parseQuery,
} from './searchQuery';
import type { Filters, SearchMatch, SearchOpts, Sort, StatusFilter } from './types';

export const filters = $state<Filters>({
  q: '',
  status: 'all',
  sort: 'year_desc',
  project: 'all',
  tag: undefined,
  starred: undefined,
});

/// The search box string is the single source of truth; project/tag/starred/
/// status on `filters` (and the field toggles on `searchOpts`) are a cache of
/// its parse — `project:` names resolve to ids via the loaded projects list
/// (unresolved names pass through verbatim and simply match nothing).
function syncFiltersFromQuery(): ParsedQuery {
  const p = parseQuery(filters.q);
  filters.tag = p.tag ?? undefined;
  filters.starred = p.starred || undefined;
  filters.status = p.status ?? 'all';
  filters.project = p.project
    ? (projects.items.find((pr) => pr.name.toLowerCase() === p.project!.toLowerCase())?.id ??
      p.project)
    : 'all';
  for (const k of FIELD_KEYS) {
    searchOpts[k] = p.fields === null || p.fields.includes(k);
  }
  return p;
}

export const searchOpts = $state<SearchOpts>({
  title: true,
  authors: true,
  abstract: true,
  body: true,
  notes: true,
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
/// selection makes it meaningless (nothing embedded is selected).
export function semanticBlocked(): boolean {
  const noEmbedded = !SEMANTIC_FIELD_KEYS.some((f) => searchOpts[f]);
  return noEmbedded || !searchMeta.semantic.available;
}

export function toggleSearchField(k: FieldKey): void {
  const on = FIELD_KEYS.filter((f) => searchOpts[f]);
  if (searchOpts[k] && on.length === 1) return; // keep at least one field
  const next = searchOpts[k] ? on.filter((f) => f !== k) : [...on, k];
  filters.q = setFieldQualifiers(filters.q, next.length === FIELD_KEYS.length ? null : next);
  if (hasSearchTerms(syncFiltersFromQuery())) void loadPapers();
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

/// Pill clicks and the status control edit the query string; the parse then
/// round-trips into the cached filters. Filters combine (AND) — the old
/// project/tag/star mutual exclusivity is gone by design. Also the write path
/// for tag/project rename+delete, which rewrite the qualifier they own.
export async function applyQueryEdit(q: string): Promise<void> {
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  filters.q = q;
  syncFiltersFromQuery();
  await loadPapers();
}

export async function setProjectFilter(id: string): Promise<void> {
  const name = id === 'all' ? null : (projects.items.find((p) => p.id === id)?.name ?? id);
  await applyQueryEdit(setQualifier(filters.q, 'project', name));
}

export async function setTagFilter(tag: string | undefined): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'tag', tag ?? null));
}

export async function setStarFilter(on: boolean): Promise<void> {
  await applyQueryEdit(setStarredQualifier(filters.q, on));
}

export async function setStatusFilter(status: StatusFilter): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'status', status === 'all' ? null : status));
}

/// Sort lives on `filters` directly (it's a query param, not a search-string
/// qualifier), but changing it still means "mutate + reload" like the setters
/// above — the sort selects in FilterRow and LibraryTable both come here.
export async function setSortFilter(sort: Sort): Promise<void> {
  filters.sort = sort;
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

/// Human-readable names for every non-default list filter — what an empty
/// state should blame ("No papers match X · Y"). Shared by the list pane
/// and the main-area empty state.
export function activeFilterLabels(): string[] {
  const labels: string[] = [];
  if (filters.q.trim()) labels.push(`“${filters.q.trim()}”`);
  if (filters.project !== 'all')
    labels.push(projects.items.find((p) => p.id === filters.project)?.name ?? 'the selected project');
  if (filters.tag) labels.push(filters.tag);
  if (filters.starred !== undefined) labels.push('starred');
  if (filters.status !== 'all')
    labels.push(filters.status === 'needs_review' ? 'needs review' : filters.status);
  return labels;
}

/// The filtered-empty sentence itself, shared verbatim by the list pane and
/// the main-area hero so the two always tell the same story (each site keeps
/// its own layout and Clear-filters button).
export function noMatchesBlame(): string {
  return `No papers match ${activeFilterLabels().join(' · ')}.`;
}

/// Reset every list filter (search, status, project/tag/star) to the default
/// view and reload — the escape hatch offered by the list's empty state.
export async function clearFilters(): Promise<void> {
  filters.q = '';
  syncFiltersFromQuery(); // empty q → all filters default, all fields on
  await loadPapers();
}

let kwDebounce: ReturnType<typeof setTimeout> | undefined;
let fullDebounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  const parsed = syncFiltersFromQuery();
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  if (!hasSearchTerms(parsed)) {
    // Qualifier-only (or empty) → plain list; small debounce so typing a
    // qualifier character-by-character doesn't fire a request per keystroke.
    kwDebounce = setTimeout(() => void loadPapers(), 150);
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
