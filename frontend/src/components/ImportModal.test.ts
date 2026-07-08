import { beforeEach, describe, expect, it, vi } from 'vitest';
import { enqueueFiles, importState, openImport } from '../lib/state.svelte';

function pdf(name: string): File {
  return new File([new Uint8Array([0x25, 0x50, 0x44, 0x46])], name, {
    type: 'application/pdf',
  });
}

// A fetch stub: POST /api/papers -> per-file JSON body; GET list/stats -> empty.
// The HTTP status is 400 when the body carries an `error`, else 200 (so a
// paper's own `status: 'resolved'` string never gets mistaken for an HTTP code).
function stubFetch(outcome: (name: string) => object) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const u = String(url);
      const json = (o: unknown, status = 200) =>
        new Response(JSON.stringify(o), {
          status,
          headers: { 'content-type': 'application/json' },
        });
      if (u === '/api/papers' && init?.method === 'POST') {
        const f = (init.body as FormData).get('file') as File;
        const payload = outcome(f.name) as Record<string, unknown>;
        return json(payload, typeof payload.error === 'string' ? 400 : 200);
      }
      if (u.startsWith('/api/papers')) return json([]);
      return json({ total: 0, resolved: 0, needs_review: 0 });
    }),
  );
}

describe('enqueueFiles', () => {
  beforeEach(() => {
    openImport(); // resets importState
    vi.restoreAllMocks();
  });

  it('imports files sequentially and records each outcome', async () => {
    const seen: string[] = [];
    stubFetch((name) => {
      seen.push(name);
      return name === 'a.pdf'
        ? { outcome: 'ingested', id: '1', title: 'A', status: 'resolved' }
        : { outcome: 'duplicate' };
    });

    await enqueueFiles([pdf('a.pdf'), pdf('b.pdf')]);

    expect(seen).toEqual(['a.pdf', 'b.pdf']); // one at a time, in order
    expect(importState.items.map((i) => i.status)).toEqual(['ingested', 'duplicate']);
    expect(importState.items[0].message).toBe('A');
  });

  it('marks a rejected upload as failed with the server message', async () => {
    stubFetch(() => ({ error: 'not a PDF' }));

    await enqueueFiles([pdf('bad.pdf')]);

    expect(importState.items[0].status).toBe('failed');
    expect(importState.items[0].message).toBe('not a PDF');
  });
});
