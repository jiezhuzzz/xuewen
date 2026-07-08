import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatusPill from './StatusPill.svelte';

describe('StatusPill', () => {
  it('shows "resolved" for resolved status', () => {
    render(StatusPill, { props: { status: 'resolved' } });
    expect(screen.getByText('resolved')).toBeInTheDocument();
  });

  it('shows "needs review" otherwise', () => {
    render(StatusPill, { props: { status: 'needs_review' } });
    expect(screen.getByText('needs review')).toBeInTheDocument();
  });
});
