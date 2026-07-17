import type {
  BibFormat,
  Candidate,
  Filters,
  IdentifyBody,
  ImportResult,
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
  TagSummary,
} from './types';

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
  throw new Error(msg);
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
  const res = await fetch(`/api/papers?${filterParams(f).toString()}`);
  if (!res.ok) throw new Error(`list failed: ${res.status}`);
  return res.json();
}

export async function getPaper(id: string): Promise<PaperDetail> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}`);
  if (!res.ok) throw new Error(`detail failed: ${res.status}`);
  return res.json();
}

export async function getStats(): Promise<Stats> {
  const res = await fetch('/api/stats');
  if (!res.ok) throw new Error(`stats failed: ${res.status}`);
  return res.json();
}

export function pdfUrl(id: string): string {
  return `/papers/${encodeURIComponent(id)}/pdf`;
}

export async function deletePaper(id: string): Promise<void> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`delete failed: ${res.status}`);
}

/// Un-trash a soft-deleted paper (the delete toast's Undo).
export async function restorePaper(id: string): Promise<void> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/restore`, { method: 'POST' });
  if (!res.ok) throw new Error(`restore failed: ${res.status}`);
}

export async function importPaper(file: File): Promise<ImportResult> {
  const body = new FormData();
  body.append('file', file, file.name);
  const res = await fetch('/api/papers', { method: 'POST', body });
  if (!res.ok) return errorFromResponse(res, 'import failed');
  return res.json();
}

export async function importUrl(input: string): Promise<ImportResult> {
  const res = await fetch('/api/import', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ input }),
  });
  if (!res.ok) return errorFromResponse(res, 'import failed');
  return res.json();
}

export async function getSettings(): Promise<Settings> {
  const res = await fetch('/api/settings');
  if (!res.ok) throw new Error(`settings failed: ${res.status}`);
  return res.json();
}

export async function translateText(
  text: string,
  opts?: { provider?: 'llm' | 'deepl'; targetLang?: string },
): Promise<{ translation: string; provider: string; source_lang: string | null; target_lang: string }> {
  const res = await fetch('/api/translate', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ text, provider: opts?.provider, target_lang: opts?.targetLang }),
  });
  if (!res.ok) throw await errorFromResponse(res, 'translate failed');
  return res.json();
}

export async function setProxyCookie(cookie: string): Promise<void> {
  const res = await fetch('/api/settings/proxy-cookie', {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ cookie }),
  });
  if (!res.ok) throw new Error(`save cookie failed: ${res.status}`);
}

export async function clearProxyCookie(): Promise<void> {
  const res = await fetch('/api/settings/proxy-cookie', { method: 'DELETE' });
  if (!res.ok) throw new Error(`clear cookie failed: ${res.status}`);
}

export async function identifySearch(q: string): Promise<Candidate[]> {
  const res = await fetch(`/api/identify/search?q=${encodeURIComponent(q)}`);
  if (!res.ok) throw new Error(`search failed: ${res.status}`);
  return res.json();
}

export async function identifyPaper(id: string, body: IdentifyBody): Promise<PaperDetail> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/identify`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) return errorFromResponse(res, 'identify failed');
  return res.json();
}

export async function listProjects(): Promise<Project[]> {
  const res = await fetch('/api/projects');
  if (!res.ok) throw new Error(`projects failed: ${res.status}`);
  return res.json();
}

export async function createProject(name: string): Promise<Project> {
  const res = await fetch('/api/projects', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) return errorFromResponse(res, 'create project failed');
  return res.json();
}

export async function updateProject(id: string, patch: { name?: string }): Promise<Project> {
  const res = await fetch(`/api/projects/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(patch),
  });
  if (!res.ok) return errorFromResponse(res, 'update project failed');
  return res.json();
}

export async function deleteProject(id: string): Promise<void> {
  const res = await fetch(`/api/projects/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`delete project failed: ${res.status}`);
}

export async function addPaperToProject(paperId: string, projectId: string): Promise<void> {
  const res = await fetch(
    `/api/papers/${encodeURIComponent(paperId)}/projects/${encodeURIComponent(projectId)}`,
    { method: 'PUT' },
  );
  if (!res.ok) throw new Error(`add to project failed: ${res.status}`);
}

export async function removePaperFromProject(paperId: string, projectId: string): Promise<void> {
  const res = await fetch(
    `/api/papers/${encodeURIComponent(paperId)}/projects/${encodeURIComponent(projectId)}`,
    { method: 'DELETE' },
  );
  if (!res.ok) throw new Error(`remove from project failed: ${res.status}`);
}

export async function listTags(): Promise<TagSummary[]> {
  const res = await fetch('/api/tags');
  if (!res.ok) throw new Error(`tags failed: ${res.status}`);
  return res.json();
}

export async function addTag(
  paperId: string,
  name: string,
): Promise<{ id: string; name: string }> {
  const res = await fetch(`/api/papers/${encodeURIComponent(paperId)}/tags`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) return errorFromResponse(res, 'add tag failed');
  return res.json();
}

export async function removeTag(paperId: string, tagId: string): Promise<void> {
  const res = await fetch(
    `/api/papers/${encodeURIComponent(paperId)}/tags/${encodeURIComponent(tagId)}`,
    { method: 'DELETE' },
  );
  if (!res.ok) throw new Error(`remove tag failed: ${res.status}`);
}

export async function renameTag(id: string, name: string): Promise<void> {
  const res = await fetch(`/api/tags/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) throw new Error(`rename tag failed: ${res.status}`);
}

export async function deleteTag(id: string): Promise<void> {
  const res = await fetch(`/api/tags/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`delete tag failed: ${res.status}`);
}

export async function setStar(paperId: string, on: boolean): Promise<void> {
  const res = await fetch(`/api/papers/${encodeURIComponent(paperId)}/star`, {
    method: on ? 'PUT' : 'DELETE',
  });
  if (!res.ok) throw new Error(`star failed: ${res.status}`);
}

export async function exportPaper(id: string, fmt: BibFormat): Promise<string> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/export?format=${fmt}`);
  if (!res.ok) throw new Error(`export failed: ${res.status}`);
  return res.text();
}

export function exportUrl(f: Filters, fmt: BibFormat): string {
  const params = filterParams(f);
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
  const res = await fetch(`/api/search?${searchParams(q, opts, keywordOnly).toString()}`);
  if (!res.ok) throw new Error(`search failed: ${res.status}`);
  return res.json();
}

export async function getSearchStatus(): Promise<SearchStatus> {
  const res = await fetch('/api/search/status');
  if (!res.ok) throw new Error(`search status failed: ${res.status}`);
  return res.json();
}

// --- chat (HTTP layer only; session/stream bookkeeping lives in chat.svelte.ts) ---

export async function getChatModels(): Promise<{ available: boolean; models: unknown[] }> {
  const res = await fetch('/api/chat/models');
  if (!res.ok) throw new Error(`chat models failed: ${res.status}`);
  return res.json();
}

export async function getChatThread(paperId: string): Promise<unknown[]> {
  const res = await fetch(`/api/papers/${encodeURIComponent(paperId)}/chat`);
  if (!res.ok) throw new Error(`chat thread failed: ${res.status}`);
  return res.json();
}

export async function deleteChatThread(paperId: string): Promise<void> {
  const res = await fetch(`/api/papers/${encodeURIComponent(paperId)}/chat`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`clear chat failed: ${res.status}`);
}

/** POST a chat message; returns the raw streaming Response (SSE body) —
 *  the only endpoint whose caller needs the stream, so it alone hands the
 *  Response back instead of parsed JSON. */
export async function postChatMessage(
  paperId: string,
  body: { model_id: string | null; message: string },
  signal: AbortSignal,
): Promise<Response> {
  const res = await fetch(`/api/papers/${encodeURIComponent(paperId)}/chat`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal,
  });
  if (!res.ok || !res.body) throw new Error(`request failed (${res.status})`);
  return res;
}

// --- code (attach-a-repo for the agent; Task 6 on the backend) ---

export async function getPaperCode(
  id: string,
): Promise<{ attached: boolean; code: PaperCodeStatus | null }> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/code`);
  if (!res.ok) return errorFromResponse(res, 'loading the code status failed');
  return res.json();
}

export async function setPaperCode(
  id: string,
  repoUrl: string,
): Promise<{ attached: boolean; code: PaperCodeStatus | null }> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/code`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ repo_url: repoUrl }),
  });
  if (!res.ok) return errorFromResponse(res, 'attaching the repo failed');
  return res.json();
}

export async function removePaperCode(id: string): Promise<void> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/code`, { method: 'DELETE' });
  if (!res.ok) return errorFromResponse(res, 'removing the repo failed');
}

/** Parse extracted reference strings via the backend LLM service. Returns
 *  null on ANY failure (503 = [ai.citations] unconfigured, network error,
 *  unexpected shape) — the popover then just keeps showing raw text. */
export async function parseCitations(
  paperId: string,
  references: string[],
): Promise<(StructuredReference | null)[] | null> {
  try {
    const res = await fetch(`/api/papers/${encodeURIComponent(paperId)}/citations`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ references }),
    });
    if (!res.ok) return null;
    const j = await res.json();
    return Array.isArray(j.references) ? j.references : null;
  } catch {
    return null;
  }
}
