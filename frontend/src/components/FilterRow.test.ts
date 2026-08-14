import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import FilterRow from './FilterRow.svelte';
import { filters, projects, tags } from '../lib/state.svelte';

const project = { id: 'pr1', name: 'NLP', paper_count: 3 };
const tag = { id: 't1', name: 'nlp/eval', paper_count: 2, created_at: '' };

// The tag/project lists must come from the stub — the mount-time loadTags()
// (and the loadProjects()/loadTags() reloads after a mutation) overwrite the
// stores with whatever the API returns. Other GETs return a list; mutations
// only need an ok JSON body.
function stubFetch() {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, opts?: { method?: string }) => {
      const u = String(url);
      const method = opts?.method ?? 'GET';
      let body = method === 'GET' ? '[]' : '{}';
      if (method === 'GET' && u.includes('/api/tags')) body = JSON.stringify([tag]);
      if (method === 'GET' && u.includes('/api/projects')) body = JSON.stringify([project]);
      return new Response(body, { status: 200, headers: { 'content-type': 'application/json' } });
    }),
  );
}

function fetchCalls(): [string, { method?: string } | undefined][] {
  return (globalThis.fetch as ReturnType<typeof vi.fn>).mock.calls as never;
}

beforeEach(() => {
  filters.q = '';
  filters.status = 'all';
  filters.sort = 'year_desc';
  filters.project = 'all';
  filters.tag = undefined;
  filters.starred = undefined;
  projects.items = [project];
  tags.items = [tag];
  vi.unstubAllGlobals();
  stubFetch();
});

async function openProjects() {
  await userEvent.click(screen.getByRole('button', { name: 'Projects' }));
}

async function openStarTags() {
  await userEvent.click(screen.getByRole('button', { name: 'Star & tags' }));
}

describe('FilterRow pill context menu', () => {
  it('keeps the "⋯" trigger out of layout (sr-only) until keyboard focus', async () => {
    render(FilterRow);
    await openProjects();
    const trigger = screen.getByRole('button', { name: 'NLP options' });
    expect(trigger.className).toContain('sr-only');
    expect(trigger.className).toContain('focus-visible:not-sr-only');
  });

  it('activating the keyboard trigger opens the menu', async () => {
    render(FilterRow);
    await openProjects();
    await userEvent.click(screen.getByRole('button', { name: 'NLP options' }));
    expect(screen.getByRole('menu', { name: 'NLP options' })).toBeInTheDocument();
  });

  it('right-clicking a project pill opens the menu and focuses the first action', async () => {
    render(FilterRow);
    await openProjects();
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'NLP 3' }));
    expect(screen.getByRole('menu', { name: 'NLP options' })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('menuitem', { name: 'Rename' })).toHaveFocus());
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toBeInTheDocument();
  });

  it('left-clicking a pill still toggles the filter, not the menu', async () => {
    render(FilterRow);
    await openProjects();
    await userEvent.click(screen.getByRole('button', { name: 'NLP 3' }));
    await waitFor(() => expect(filters.project).toBe('pr1'));
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('renames a project through the menu', async () => {
    render(FilterRow);
    await openProjects();
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'NLP 3' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    const input = screen.getByRole('textbox', { name: 'Rename NLP' });
    expect(input).toHaveValue('NLP');
    await userEvent.clear(input);
    await userEvent.type(input, 'NLU{Enter}');
    await waitFor(() => {
      expect(
        fetchCalls().some(
          ([u, o]) => String(u).includes('/api/projects/pr1') && o?.method === 'PATCH',
        ),
      ).toBe(true);
    });
    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument());
  });

  it('deleting a tag needs a confirm before the DELETE fires', async () => {
    render(FilterRow);
    await openStarTags();
    await fireEvent.contextMenu(await screen.findByRole('button', { name: 'nlp/eval 2' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    // First click only reveals the confirm — no DELETE yet.
    expect(fetchCalls().some(([, o]) => o?.method === 'DELETE')).toBe(false);
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(
        fetchCalls().some(
          ([u, o]) => String(u).includes('/api/tags/t1') && o?.method === 'DELETE',
        ),
      ).toBe(true);
    });
    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument());
  });

  it('Escape steps back from the delete confirm, then closes the menu', async () => {
    render(FilterRow);
    await openProjects();
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'NLP 3' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    await userEvent.keyboard('{Escape}');
    expect(screen.getByRole('menuitem', { name: 'Rename' })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('a successful delete does not leave the next rename stuck busy', async () => {
    render(FilterRow);
    await openStarTags();
    await fireEvent.contextMenu(await screen.findByRole('button', { name: 'nlp/eval 2' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    await userEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument());
    await openProjects();
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'NLP 3' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    expect(screen.getByRole('button', { name: 'Save' })).not.toBeDisabled();
  });

  it('right-clicking another pill moves the menu to it', async () => {
    projects.items = [project, { id: 'pr2', name: 'Agents', paper_count: 1 }];
    render(FilterRow);
    await openProjects();
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'NLP 3' }));
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'Agents 1' }));
    expect(screen.getByRole('menu', { name: 'Agents options' })).toBeInTheDocument();
    expect(screen.queryByRole('menu', { name: 'NLP options' })).not.toBeInTheDocument();
  });
});
