import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import Welcome from './Welcome.svelte';
import { library } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const paper: PaperSummary = {
  id: 'p1', title: 't', authors: [], venue: null, year: null, doi: null, arxiv_id: null,
  dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved', added_at: '',
  starred: false, tags: [], projects: [],
};

beforeEach(() => {
  library.papers = [];
});

describe('Welcome', () => {
  it('prompts to import when the library is empty', () => {
    render(Welcome);
    expect(screen.getByText(/library is empty/i)).toBeInTheDocument();
  });

  it('tells you to click a paper to read once the library has items', () => {
    library.papers = [paper];
    render(Welcome);
    expect(screen.getByText(/click a paper to read/i)).toBeInTheDocument();
  });
});
