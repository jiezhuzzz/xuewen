import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PaperRowTags from './PaperRowTags.svelte';
import { filters } from '../lib/state.svelte';
import type { PaperSummary } from '../lib/types';

const paper: PaperSummary = {
  id: 'p1',
  title: 't',
  authors: [],
  venue: null,
  year: null,
  doi: null,
  arxiv_id: null,
  dblp_key: null,
  cite_key: null,
  url: null,
  source: null,
  status: 'resolved',
  added_at: '',
  starred: false,
  projects: [],
  tags: [
    { id: 't1', name: 'security/fuzzing' },
    { id: 't2', name: 'os/rtos' },
    { id: 't3', name: 'ml/llm' },
    { id: 't4', name: 'benchmarks' },
    { id: 't5', name: 'robotics' },
  ],
};

beforeEach(() => {
  filters.tag = undefined;
});

describe('PaperRowTags', () => {
  it('caps tag chips at 3 with a +N overflow control', () => {
    render(PaperRowTags, { props: { paper } });
    expect(screen.getByText('security/fuzzing')).toBeInTheDocument();
    expect(screen.getByText('os/rtos')).toBeInTheDocument();
    expect(screen.getByText('ml/llm')).toBeInTheDocument();
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
    expect(screen.queryByText('robotics')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '+2' })).toBeInTheDocument();
  });

  it('reveals all 5 tags when the +2 control is clicked', async () => {
    render(PaperRowTags, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: '+2' }));
    expect(screen.getByText('security/fuzzing')).toBeInTheDocument();
    expect(screen.getByText('os/rtos')).toBeInTheDocument();
    expect(screen.getByText('ml/llm')).toBeInTheDocument();
    expect(screen.getByText('benchmarks')).toBeInTheDocument();
    expect(screen.getByText('robotics')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^\+\d/ })).not.toBeInTheDocument();
  });

  it('renders project badges that never count toward the tag cap', () => {
    const withProject = { ...paper, projects: [{ id: 'pr1', name: 'RTOS Fuzzing' }] };
    render(PaperRowTags, { props: { paper: withProject } });
    expect(screen.getByText('RTOS Fuzzing')).toBeInTheDocument();
    // Still only 3 tag chips are shown + a +2 (the badge is not part of the cap).
    expect(screen.getByRole('button', { name: '+2' })).toBeInTheDocument();
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
  });

  it('keeps a tag matching the active filter visible beyond the cap', () => {
    filters.tag = 'robotics';
    render(PaperRowTags, { props: { paper } });
    expect(screen.getByText('robotics')).toBeInTheDocument();
    // benchmarks (index 3, no match) stays hidden; robotics (index 4) is
    // pulled forward for the filter hit, so only 1 remains truly hidden.
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '+1' })).toBeInTheDocument();
  });
});
