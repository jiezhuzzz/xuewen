import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { projects, ui } from '../lib/state.svelte';
import ProjectsModal from './ProjectsModal.svelte';

function stubFetch(handler: (url: string, init?: RequestInit) => unknown) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const body = handler(String(url), init);
      return new Response(JSON.stringify(body ?? {}), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      });
    }),
  );
}

describe('ProjectsModal', () => {
  beforeEach(() => {
    projects.items = [{ id: 'p1', name: 'Survey', note: null, paper_count: 3 }];
    ui.projectsOpen = true;
    vi.unstubAllGlobals();
  });

  it('renders existing projects with counts', () => {
    stubFetch(() => []);
    render(ProjectsModal);
    // The name is an editable input seeded with the project name.
    expect(screen.getByDisplayValue('Survey')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('creates a project from the form', async () => {
    const calls: Array<{ url: string; method?: string }> = [];
    stubFetch((url, init) => {
      calls.push({ url, method: init?.method });
      if (url === '/api/projects' && init?.method === 'POST')
        return { id: 'p2', name: 'New', note: null, paper_count: 0 };
      return [{ id: 'p1', name: 'Survey', note: null, paper_count: 3 }];
    });
    render(ProjectsModal);
    await userEvent.type(screen.getByPlaceholderText('New project name…'), 'New');
    await userEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect(calls.some((c) => c.url === '/api/projects' && c.method === 'POST')).toBe(true);
  });

  it('renames a project via PATCH on blur when the name changed', async () => {
    const calls: Array<{ url: string; method?: string; body?: string }> = [];
    stubFetch((url, init) => {
      calls.push({ url, method: init?.method, body: init?.body as string | undefined });
      if (init?.method === 'PATCH') return { id: 'p1', name: 'Renamed', note: null, paper_count: 3 };
      return [{ id: 'p1', name: 'Renamed', note: null, paper_count: 3 }];
    });
    render(ProjectsModal);
    const nameInput = screen.getByLabelText('Rename Survey') as HTMLInputElement;
    await userEvent.clear(nameInput);
    await userEvent.type(nameInput, 'Renamed');
    // Tab away to blur the focused input, triggering the rename-on-blur.
    await userEvent.tab();
    const patch = calls.find((c) => c.method === 'PATCH');
    expect(patch).toBeTruthy();
    expect(patch?.url).toBe('/api/projects/p1');
    expect(patch?.body).toContain('Renamed');
  });

  it('confirms before deleting a project', async () => {
    const calls: Array<{ url: string; method?: string }> = [];
    stubFetch((url, init) => {
      calls.push({ url, method: init?.method });
      return [{ id: 'p1', name: 'Survey', note: null, paper_count: 3 }];
    });
    render(ProjectsModal);
    // First click reveals the inline confirm; no DELETE yet.
    await userEvent.click(screen.getByRole('button', { name: 'Delete Survey' }));
    expect(calls.some((c) => c.method === 'DELETE')).toBe(false);
    // Second click on the confirm button issues the DELETE.
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(calls.some((c) => c.url === '/api/projects/p1' && c.method === 'DELETE')).toBe(true);
  });
});
