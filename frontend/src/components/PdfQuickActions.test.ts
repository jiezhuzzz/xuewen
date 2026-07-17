import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PdfQuickActions from './PdfQuickActions.svelte';
import { chat } from '../lib/chat.svelte';
import { dock, ui, viewer } from '../lib/state.svelte';
import type { PillHide } from '../lib/pillHide.svelte';

/// A plain fake matching the real `PillHide` interface (see
/// `lib/pillHide.svelte.ts`) — `createPillHide` registers `$effect`s and is
/// meant to be constructed during a component's own init, so tests build the
/// shape directly instead of calling the factory standalone.
function makePill(): PillHide {
  return {
    visible: true,
    setHost() {},
    setExtraHold() {},
    onWindowMove() {},
    pillEnter() {},
    pillLeave() {},
    focusIn() {},
    focusOut() {},
  };
}

beforeEach(() => {
  viewer.activeId = 'p1';
  dock.open = false;
  dock.tab = 'details';
  ui.zen = false;
  chat.available = true;
  localStorage.clear();
});

describe('PdfQuickActions seals', () => {
  it('renders 禪 詳 問 and no translate toggle', () => {
    render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.getByRole('button', { name: 'Zen mode' })).toHaveTextContent('禪');
    expect(screen.getByRole('button', { name: 'Details' })).toHaveTextContent('詳');
    expect(screen.getByRole('button', { name: 'Ask about this paper' })).toHaveTextContent('問');
    expect(screen.queryByRole('button', { name: /translate/i })).not.toBeInTheDocument();
  });

  it('hides 問 when chat is unavailable', () => {
    chat.available = false;
    render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.queryByRole('button', { name: 'Ask about this paper' })).not.toBeInTheDocument();
  });

  it('詳 and 問 open the dock on their tabs', async () => {
    render(PdfQuickActions, { props: { pill: makePill() } });
    await userEvent.click(screen.getByRole('button', { name: 'Details' }));
    expect(dock.open).toBe(true);
    expect(dock.tab).toBe('details');
    await userEvent.click(screen.getByRole('button', { name: 'Ask about this paper' }));
    expect(dock.tab).toBe('ask');
  });

  it('禪 toggles zen', async () => {
    render(PdfQuickActions, { props: { pill: makePill() } });
    await userEvent.click(screen.getByRole('button', { name: 'Zen mode' }));
    expect(ui.zen).toBe(true);
  });

  it('the pill yields while the dock is open', () => {
    dock.open = true;
    render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.getByRole('toolbar', { name: 'Reader quick actions' }).className).toContain('opacity-0');
  });
});
