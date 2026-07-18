import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../lib/api', async (importOriginal) => {
  const mod = await importOriginal<typeof import('../lib/api')>();
  return { ...mod, listPapers: vi.fn(async () => []) };
});

import * as api from '../lib/api';
import PaperList from './PaperList.svelte';
import { filters, library, projects } from '../lib/state.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  library.loading = false;
  library.error = null;
  library.papers = [];
  projects.items = [{ id: 'pr1', name: 'RTOS Fuzzing', paper_count: 0 }];
  Object.assign(filters, {
    q: '',
    status: 'all',
    sort: 'year_desc',
    project: 'all',
    tag: undefined,
    starred: undefined,
  });
});

describe('PaperList loading state', () => {
  it('shows a spinner while loading', () => {
    library.loading = true;
    render(PaperList);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });
});

describe('PaperList empty state', () => {
  it('names the active project filter and clears it on click', async () => {
    filters.project = 'pr1';
    render(PaperList);
    expect(screen.getByText(/RTOS Fuzzing/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(filters.project).toBe('all');
    expect(api.listPapers).toHaveBeenCalled();
  });

  it('names an active search and tag filter', () => {
    filters.q = 'fuzzing';
    filters.tag = 'os/rtos';
    render(PaperList);
    expect(screen.getByText(/“fuzzing”/)).toBeInTheDocument();
    expect(screen.getByText(/os\/rtos/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Clear filters' })).toBeInTheDocument();
  });

  it('offers import instead of clear when no filter is active', () => {
    render(PaperList);
    expect(screen.getByText(/empty/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Clear filters' })).not.toBeInTheDocument();
  });
});
