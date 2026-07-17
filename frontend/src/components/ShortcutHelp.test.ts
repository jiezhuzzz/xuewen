import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import ShortcutHelp from './ShortcutHelp.svelte';
import { ui } from '../lib/state.svelte';

beforeEach(() => {
  ui.helpOpen = true;
});

describe('ShortcutHelp', () => {
  it('lists the app shortcuts', () => {
    render(ShortcutHelp);
    expect(screen.getByText('Search library')).toBeInTheDocument();
    expect(screen.getByText('Zen mode')).toBeInTheDocument();
    expect(screen.getByText('Command palette')).toBeInTheDocument();
    expect(screen.getByText('Next / previous paper')).toBeInTheDocument();
    expect(screen.getByText('Find in PDF')).toBeInTheDocument();
  });

  it('closes on Escape', async () => {
    render(ShortcutHelp);
    await userEvent.keyboard('{Escape}');
    expect(ui.helpOpen).toBe(false);
  });
});
