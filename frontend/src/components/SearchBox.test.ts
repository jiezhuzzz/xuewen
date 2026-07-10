import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import SearchBox from './SearchBox.svelte';
import { filters, searchMeta, searchOpts } from '../lib/state.svelte';

beforeEach(() => {
  filters.q = '';
  searchOpts.title = true;
  searchOpts.authors = true;
  searchOpts.abstract = true;
  searchOpts.body = true;
  searchOpts.keyword = true;
  searchOpts.semantic = true;
  searchMeta.semantic = { available: true, reason: null };
  searchMeta.pending = 0;
});

describe('SearchBox options popover', () => {
  it('opens with the options button and closes on Escape', async () => {
    render(SearchBox);
    await userEvent.click(screen.getByRole('button', { name: 'Search options' }));
    expect(screen.getByText('Engines')).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByText('Engines')).not.toBeInTheDocument();
  });

  it('closes when clicking outside', async () => {
    render(SearchBox);
    await userEvent.click(screen.getByRole('button', { name: 'Search options' }));
    expect(screen.getByText('Engines')).toBeInTheDocument();
    await userEvent.click(document.body);
    expect(screen.queryByText('Engines')).not.toBeInTheDocument();
  });
});

describe('SearchBox narrowed hint', () => {
  it('is hidden when every available option is on', () => {
    render(SearchBox);
    expect(screen.queryByText(/Search options narrowed/)).not.toBeInTheDocument();
  });

  it('does not count server-blocked semantic as narrowed', () => {
    searchMeta.semantic = { available: false, reason: 'semantic search not configured' };
    render(SearchBox);
    expect(screen.queryByText(/Search options narrowed/)).not.toBeInTheDocument();
  });

  it('appears when the user turns a field off', async () => {
    render(SearchBox);
    await userEvent.click(screen.getByRole('button', { name: 'Search options' }));
    await userEvent.click(screen.getByRole('button', { name: 'Body' }));
    await userEvent.keyboard('{Escape}'); // hint only renders while the popover is closed
    expect(screen.getByText(/Search options narrowed/)).toBeInTheDocument();
  });
});
