import type {
  BibFormat,
  Candidate,
  Filters,
  IdentifyBody,
  ImportResult,
  PaperDetail,
  PaperSummary,
  Project,
  Settings,
  Stats,
} from './types';

export async function listPapers(f: Filters): Promise<PaperSummary[]> {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  params.set('sort', f.sort);
  if (f.project && f.project !== 'all') params.set('project', f.project);
  const res = await fetch(`/api/papers?${params.toString()}`);
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

export async function importPaper(file: File): Promise<ImportResult> {
  const body = new FormData();
  body.append('file', file, file.name);
  const res = await fetch('/api/papers', { method: 'POST', body });
  if (!res.ok) {
    let msg = `import failed: ${res.status}`;
    try {
      const j = await res.json();
      if (j && typeof j.error === 'string') msg = j.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(msg);
  }
  return res.json();
}

export async function importUrl(input: string): Promise<ImportResult> {
  const res = await fetch('/api/import', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ input }),
  });
  if (!res.ok) {
    let msg = `import failed: ${res.status}`;
    try {
      const j = await res.json();
      if (j && typeof j.error === 'string') msg = j.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(msg);
  }
  return res.json();
}

export async function getSettings(): Promise<Settings> {
  const res = await fetch('/api/settings');
  if (!res.ok) throw new Error(`settings failed: ${res.status}`);
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
  if (!res.ok) {
    let msg = `identify failed: ${res.status}`;
    try {
      const j = await res.json();
      if (j && typeof j.error === 'string') msg = j.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(msg);
  }
  return res.json();
}

export async function listProjects(): Promise<Project[]> {
  const res = await fetch('/api/projects');
  if (!res.ok) throw new Error(`projects failed: ${res.status}`);
  return res.json();
}

async function projectError(res: Response, fallback: string): Promise<never> {
  let msg = `${fallback}: ${res.status}`;
  try {
    const j = await res.json();
    if (j && typeof j.error === 'string') msg = j.error;
  } catch {
    /* non-JSON error body */
  }
  throw new Error(msg);
}

export async function createProject(name: string, note: string | null): Promise<Project> {
  const res = await fetch('/api/projects', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name, note }),
  });
  if (!res.ok) return projectError(res, 'create project failed');
  return res.json();
}

export async function updateProject(
  id: string,
  patch: { name?: string; note?: string | null },
): Promise<Project> {
  const res = await fetch(`/api/projects/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(patch),
  });
  if (!res.ok) return projectError(res, 'update project failed');
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

export async function exportPaper(id: string, fmt: BibFormat): Promise<string> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/export?format=${fmt}`);
  if (!res.ok) throw new Error(`export failed: ${res.status}`);
  return res.text();
}

export function exportUrl(f: Filters, fmt: BibFormat): string {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  if (f.project && f.project !== 'all') params.set('project', f.project);
  params.set('sort', f.sort);
  params.set('format', fmt);
  return `/api/papers/export?${params.toString()}`;
}
