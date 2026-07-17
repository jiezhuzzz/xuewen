import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import Toaster from './Toaster.svelte';
import { toast, toasts } from '../lib/toasts.svelte';

beforeEach(() => {
  toasts.items = [];
});

describe('Toaster', () => {
  it('announces error toasts assertively via role=alert', () => {
    toast('error', 'Delete failed', 0);
    render(Toaster);
    expect(screen.getByRole('alert')).toHaveTextContent('Delete failed');
  });

  it('keeps non-error toasts polite (no alert role)', () => {
    toast('success', 'Saved', 0);
    render(Toaster);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getByText('Saved')).toBeInTheDocument();
  });
});
