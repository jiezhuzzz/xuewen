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
    const { findByText, queryByRole } = render(CitationPopover);
    expect(await findByText(/Adam\. ICLR 2015\./)).toBeInTheDocument();
    expect(queryByRole('button', { name: /open in library/i })).not.toBeInTheDocument();
    expect(queryByRole('link')).not.toBeInTheDocument();
  });

  it('shows Open in library only when matched', async () => {
    show({}, paper);
    const { findByRole, queryByRole } = render(CitationPopover);
    expect(await findByRole('button', { name: /open in library/i })).toBeInTheDocument();
    expect(queryByRole('link')).not.toBeInTheDocument();
  });

  it('shows an external link when the entry has a url', async () => {
    show({ externalUrl: 'https://doi.org/10.1/x' }, null);
    const { findByRole, queryByRole } = render(CitationPopover);
    const link = await findByRole('link', { name: /doi\.org/i });
    expect(link).toHaveAttribute('href', 'https://doi.org/10.1/x');
    expect(queryByRole('button', { name: /open in library/i })).not.toBeInTheDocument();
  });

  it('renders a structured card when the reference is parsed', async () => {
    show(
      {
        structured: {
          authors: ['D. Kingma', 'J. Ba'], title: 'Adam: A Method for Stochastic Optimization',
          venue: 'ICLR', year: 2015,
          doi: null, arxiv_id: '1412.6980', url: null,
        },
      },
      null,
    );
    const { findByText, queryByText, findByRole } = render(CitationPopover);
    expect(await findByText('Adam: A Method for Stochastic Optimization')).toBeInTheDocument();
    expect(await findByText(/D\. Kingma, J\. Ba/)).toBeInTheDocument();
    expect(await findByText(/ICLR.*2015/)).toBeInTheDocument(); // venue (via abbreviateVenue) + year
    expect(await findByRole('link', { name: /arXiv/i })).toBeInTheDocument();
    expect(queryByText('raw ref text')).not.toBeInTheDocument(); // structured replaces raw
  });

  it('falls back to raw text when structured is null', async () => {
    show({ structured: null }, null);
    const { findByText } = render(CitationPopover);
    expect(await findByText('raw ref text')).toBeInTheDocument();
  });
});
