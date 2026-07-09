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
    expect(screen.getByText('Survey')).toBeInTheDocument();
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
});
