import { identifyPaper, identifySearch } from './api';
import { invalidateLibraryTitleIndex } from './citationMatch';
import { loadPapers, loadStats, setDetail } from './library.svelte';
import { viewer } from './tabs.svelte';
import type { Candidate, IdentifyBody } from './types';

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
    setDetail(id, detail);
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
