import type {
  Candidate,
  Filters,
  IdentifyBody,
  ImportResult,
  PaperDetail,
  PaperSummary,
  Stats,
} from './types';

export async function listPapers(f: Filters): Promise<PaperSummary[]> {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  params.set('sort', f.sort);
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
