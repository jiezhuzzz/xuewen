import { hasSearchTerms, parseQuery } from './searchQuery';
import type {
  Annotation,
  BibFormat,
  Candidate,
  ChatModelInfo,
  ChatTurnRow,
  Filters,
  IdentifyBody,
  ImportResult,
  NewAnnotation,
  PaperCodeStatus,
  PaperDetail,
  PaperSummary,
  Project,
  SearchOpts,
  SearchResponse,
  SearchStatus,
  Settings,
  Stats,
  StructuredReference,
  Tag,
  TagSummary,
} from './types';

/** A non-2xx API response, carrying the HTTP status so callers can branch on
 *  it (e.g. 404 → the paper is gone) without parsing the message back apart. */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/** `{error: "..."}` from the response body when present, else
 *  `${fallback}: ${status}`. The one home for API error extraction. */
async function errorFromResponse(res: Response, fallback: string): Promise<never> {
  let msg = `${fallback}: ${res.status}`;
  try {
    const j = await res.json();
    if (j && typeof j.error === 'string') msg = j.error;
  } catch {
    /* non-JSON error body */
  }
  throw new ApiError(msg, res.status);
}

interface RequestOpts {
  method?: string;
  /** JSON-encoded request body, sent as `content-type: application/json`. */
  json?: unknown;
  /** Pre-encoded body (FormData upload) — the browser picks the content-type. */
  body?: BodyInit;
  signal?: AbortSignal;
}

/** fetch with every non-2xx funnelled through `errorFromResponse`, so the
 *  server's `{error}` body reaches the user from any endpoint. The variants
 *  below cover the response shapes; nothing in this file calls fetch itself. */
async function requestRaw(url: string, fallback: string, opts: RequestOpts = {}): Promise<Response> {
  const init: RequestInit = { method: opts.method, signal: opts.signal };
  if (opts.json !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(opts.json);
  } else if (opts.body !== undefined) {
    init.body = opts.body;
  }
  const res = await fetch(url, init);
  if (!res.ok) return errorFromResponse(res, fallback);
  return res;
}

async function request<T>(url: string, fallback: string, opts: RequestOpts = {}): Promise<T> {
  const res = await requestRaw(url, fallback, opts);
  return res.json();
}

/** `request` for endpoints whose success body is ignored (204/empty). */
async function requestVoid(url: string, fallback: string, opts: RequestOpts = {}): Promise<void> {
  await requestRaw(url, fallback, opts);
}

async function requestText(url: string, fallback: string): Promise<string> {
  const res = await requestRaw(url, fallback);
  return res.text();
}

/** The list-filter query params shared by /api/papers and /api/papers/export. */
function filterParams(f: Filters): URLSearchParams {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  params.set('sort', f.sort);
  if (f.project && f.project !== 'all') params.set('project', f.project);
  if (f.tag) params.set('tag', f.tag);
  if (f.starred) params.set('starred', 'true');
  return params;
}

export async function listPapers(f: Filters): Promise<PaperSummary[]> {
  return request(`/api/papers?${filterParams(f).toString()}`, 'list failed');
}

export async function getPaper(id: string): Promise<PaperDetail> {
  return request(`/api/papers/${encodeURIComponent(id)}`, 'detail failed');
}

export async function getStats(): Promise<Stats> {
  return request('/api/stats', 'stats failed');
}

export function pdfUrl(id: string): string {
  return `/papers/${encodeURIComponent(id)}/pdf`;
}

export async function deletePaper(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}`, 'delete failed', {
    method: 'DELETE',
  });
}

/// Un-trash a soft-deleted paper (the delete toast's Undo).
export async function restorePaper(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}/restore`, 'restore failed', {
    method: 'POST',
  });
}

export async function importPaper(file: File): Promise<ImportResult> {
  const body = new FormData();
  body.append('file', file, file.name);
  return request('/api/papers', 'import failed', { method: 'POST', body });
}

export async function importUrl(input: string): Promise<ImportResult> {
  return request('/api/import', 'import failed', { method: 'POST', json: { input } });
}

export async function getSettings(): Promise<Settings> {
  return request('/api/settings', 'settings failed');
}

export async function translateText(
  text: string,
  opts?: { provider?: 'llm' | 'deepl'; targetLang?: string },
): Promise<{ translation: string; provider: string; source_lang: string | null; target_lang: string }> {
  return request('/api/translate', 'translate failed', {
    method: 'POST',
    json: { text, provider: opts?.provider, target_lang: opts?.targetLang },
  });
}

export async function setProxyCookie(cookie: string): Promise<void> {
  return requestVoid('/api/settings/proxy-cookie', 'save cookie failed', {
    method: 'PUT',
    json: { cookie },
  });
}

export async function clearProxyCookie(): Promise<void> {
  return requestVoid('/api/settings/proxy-cookie', 'clear cookie failed', { method: 'DELETE' });
}

export async function identifySearch(q: string): Promise<Candidate[]> {
  return request(`/api/identify/search?q=${encodeURIComponent(q)}`, 'search failed');
}

export async function identifyPaper(id: string, body: IdentifyBody): Promise<PaperDetail> {
  return request(`/api/papers/${encodeURIComponent(id)}/identify`, 'identify failed', {
    method: 'POST',
    json: body,
  });
}

export async function listProjects(): Promise<Project[]> {
  return request('/api/projects', 'projects failed');
}

export async function createProject(name: string): Promise<Project> {
  return request('/api/projects', 'create project failed', { method: 'POST', json: { name } });
}

export async function updateProject(id: string, patch: { name?: string }): Promise<Project> {
  return request(`/api/projects/${encodeURIComponent(id)}`, 'update project failed', {
    method: 'PATCH',
    json: patch,
  });
}

export async function deleteProject(id: string): Promise<void> {
  return requestVoid(`/api/projects/${encodeURIComponent(id)}`, 'delete project failed', {
    method: 'DELETE',
  });
}

export async function addPaperToProject(paperId: string, projectId: string): Promise<void> {
  return requestVoid(
    `/api/papers/${encodeURIComponent(paperId)}/projects/${encodeURIComponent(projectId)}`,
    'add to project failed',
    { method: 'PUT' },
  );
}

export async function removePaperFromProject(paperId: string, projectId: string): Promise<void> {
  return requestVoid(
    `/api/papers/${encodeURIComponent(paperId)}/projects/${encodeURIComponent(projectId)}`,
    'remove from project failed',
    { method: 'DELETE' },
  );
}

export async function listTags(): Promise<TagSummary[]> {
  return request('/api/tags', 'tags failed');
}

export async function addTag(paperId: string, name: string): Promise<Tag> {
  return request(`/api/papers/${encodeURIComponent(paperId)}/tags`, 'add tag failed', {
    method: 'PUT',
    json: { name },
  });
}

export async function removeTag(paperId: string, tagId: string): Promise<void> {
  return requestVoid(
    `/api/papers/${encodeURIComponent(paperId)}/tags/${encodeURIComponent(tagId)}`,
    'remove tag failed',
    { method: 'DELETE' },
  );
}

/// Returns the tag as the server stored it. Rename normalizes the name
/// ('nlp / eval' → 'nlp/eval') — callers treat that echo as authoritative,
/// not their input.
export async function renameTag(id: string, name: string): Promise<Tag> {
  return request(`/api/tags/${encodeURIComponent(id)}`, 'rename tag failed', {
    method: 'PATCH',
    json: { name },
  });
}

export async function deleteTag(id: string): Promise<void> {
  return requestVoid(`/api/tags/${encodeURIComponent(id)}`, 'delete tag failed', {
    method: 'DELETE',
  });
}

/// Returns the normalized name as the server stored it (trimmed; cleared to
/// null when empty) — callers treat that echo as authoritative, not their input.
export async function setPaperName(
  paperId: string,
  name: string | null,
): Promise<{ name: string | null }> {
  return request(`/api/papers/${encodeURIComponent(paperId)}/name`, 'update name failed', {
    method: 'PATCH',
    json: { name },
  });
}

export async function setStar(paperId: string, on: boolean): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(paperId)}/star`, 'star failed', {
    method: on ? 'PUT' : 'DELETE',
  });
}

/// URL for one paper's citation export — also the Download link target
/// (sibling of `pdfUrl`; `exportUrl` below covers the filtered-list export).
export function paperExportUrl(id: string, fmt: BibFormat): string {
  return `/api/papers/${encodeURIComponent(id)}/export?format=${fmt}`;
}

export async function exportPaper(id: string, fmt: BibFormat): Promise<string> {
  return requestText(paperExportUrl(id, fmt), 'export failed');
}

/// URL for the filtered-list export. The export endpoint LIKE-matches `q`
/// against title/authors, so a qualifier-only query ('tag:nlp') must not leak
/// in as `q` — the parsed filters already carry it as tag=/project=/starred=/
/// status= (the same rule as loadPapers' listPapers call). Free-text/author
/// queries keep their `q`; LibraryPane disables the export link for those.
export function exportUrl(f: Filters, fmt: BibFormat): string {
  const params = filterParams(hasSearchTerms(parseQuery(f.q)) ? f : { ...f, q: '' });
  params.set('format', fmt);
  return `/api/papers/export?${params.toString()}`;
}

/// Query string for /api/search. The raw query string carries every filter
/// (tag:/project:/is:/status:/in:/author: qualifiers are parsed server-side);
/// only the engine selection travels as a separate param, omitted when both
/// engines are on (the server default) so URLs stay short and cacheable.
export function searchParams(q: string, opts: SearchOpts, keywordOnly = false): URLSearchParams {
  const params = new URLSearchParams();
  params.set('q', q);
  const engines = keywordOnly
    ? ['keyword']
    : (['keyword', 'semantic'] as const).filter((k) => opts[k]);
  if (engines.length > 0 && engines.length < 2) params.set('engines', engines.join(','));
  return params;
}

export async function searchPapers(
  q: string,
  opts: SearchOpts,
  keywordOnly = false,
): Promise<SearchResponse> {
  return request(`/api/search?${searchParams(q, opts, keywordOnly).toString()}`, 'search failed');
}

export async function getSearchStatus(): Promise<SearchStatus> {
  return request('/api/search/status', 'search status failed');
}

// --- chat (HTTP layer only; session/stream bookkeeping lives in chat.svelte.ts) ---

export async function getChatModels(): Promise<{ available: boolean; models: ChatModelInfo[] }> {
  return request('/api/chat/models', 'chat models failed');
}

export async function getChatThread(paperId: string): Promise<ChatTurnRow[]> {
  return request(`/api/papers/${encodeURIComponent(paperId)}/chat`, 'chat thread failed');
}

export async function deleteChatThread(paperId: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(paperId)}/chat`, 'clear chat failed', {
    method: 'DELETE',
  });
}

/** POST a chat message; returns the raw streaming Response (SSE body) —
 *  the only endpoint whose caller needs the stream, so it alone hands the
 *  Response back instead of parsed JSON. */
export async function postChatMessage(
  paperId: string,
  body: { model_id: string | null; message: string },
  signal: AbortSignal,
): Promise<Response> {
  const res = await requestRaw(
    `/api/papers/${encodeURIComponent(paperId)}/chat`,
    'chat request failed',
    { method: 'POST', json: body, signal },
  );
  if (!res.body) throw new Error('chat request failed: empty response body');
  return res;
}

// --- code (attach-a-repo for the agent; Task 6 on the backend) ---

export async function getPaperCode(
  id: string,
): Promise<{ attached: boolean; code: PaperCodeStatus | null }> {
  return request(`/api/papers/${encodeURIComponent(id)}/code`, 'loading the code status failed');
}

export async function setPaperCode(
  id: string,
  repoUrl: string,
): Promise<{ attached: boolean; code: PaperCodeStatus | null }> {
  return request(`/api/papers/${encodeURIComponent(id)}/code`, 'attaching the repo failed', {
    method: 'PUT',
    json: { repo_url: repoUrl },
  });
}

export async function removePaperCode(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}/code`, 'removing the repo failed', {
    method: 'DELETE',
  });
}

// --- annotations (highlights, underlines, sticky notes) ---

function annotationUrl(paperId: string, annotationId?: string): string {
  const base = `/api/papers/${encodeURIComponent(paperId)}/annotations`;
  return annotationId === undefined ? base : `${base}/${encodeURIComponent(annotationId)}`;
}

export async function listAnnotations(paperId: string): Promise<Annotation[]> {
  return request(annotationUrl(paperId), 'loading annotations failed');
}

/** Create or replace one mark. Idempotent: the id addresses the row, so a
 *  retried save after a dropped response can't duplicate it. */
export async function putAnnotation(
  paperId: string,
  annotationId: string,
  body: NewAnnotation,
): Promise<Annotation> {
  return request(annotationUrl(paperId, annotationId), 'saving the annotation failed', {
    method: 'PUT',
    json: body,
  });
}

export async function deleteAnnotation(paperId: string, annotationId: string): Promise<void> {
  return requestVoid(annotationUrl(paperId, annotationId), 'deleting the annotation failed', {
    method: 'DELETE',
  });
}

/** Parse extracted reference strings via the backend LLM service. Returns
 *  null on ANY failure (503 = [ai.citations] unconfigured, network error,
 *  unexpected shape) — the popover then just keeps showing raw text. */
export async function parseCitations(
  paperId: string,
  references: string[],
): Promise<(StructuredReference | null)[] | null> {
  try {
    const j = await request<{ references?: (StructuredReference | null)[] }>(
      `/api/papers/${encodeURIComponent(paperId)}/citations`,
      'parsing citations failed',
      { method: 'POST', json: { references } },
    );
    return Array.isArray(j.references) ? j.references : null;
  } catch {
    return null;
  }
}
