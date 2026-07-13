import { render } from '@testing-library/svelte';
import { describe, expect, it, afterEach } from 'vitest';
import CitationPopover from './CitationPopover.svelte';
import { citationHover } from '../lib/citationState.svelte';
import type { PaperSummary } from '../lib/types';

afterEach(() => {
  citationHover.current = null;
});

const paper: PaperSummary = {
  id: 'paper-1', title: 'Adam', authors: [], venue: null, year: null, doi: null,
  arxiv_id: null, dblp_key: null, cite_key: 'k2015adam', url: null, source: null,
  status: 'resolved', added_at: '2020-01-01',
};

function show(ref: Partial<import('../lib/citations').Reference>, matchedPaper: PaperSummary | null) {
  citationHover.current = {
    reference: { index: 0, destPageIndex: 1, destY: 100, rawText: 'raw ref text', ...ref },
    matchedPaper,
    screenX: 10,
    screenY: 10,
  };
}

describe('CitationPopover', () => {
  it('always shows the raw reference text', async () => {
    show({ rawText: 'Kingma & Ba. Adam. ICLR 2015.' }, null);
    const { findByText } = render(CitationPopover);
    expect(await findByText(/Adam\. ICLR 2015\./)).toBeInTheDocument();
  });

  it('shows Open in library only when matched', async () => {
    show({}, paper);
    const { findByRole } = render(CitationPopover);
    expect(await findByRole('button', { name: /open in library/i })).toBeInTheDocument();
  });

  it('shows an external link when the entry has a url', async () => {
    show({ externalUrl: 'https://doi.org/10.1/x' }, null);
    const { findByRole } = render(CitationPopover);
    const link = await findByRole('link', { name: /doi\.org/i });
    expect(link).toHaveAttribute('href', 'https://doi.org/10.1/x');
  });
});
