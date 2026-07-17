import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PdfQuickActions from './PdfQuickActions.svelte';
import { appSettings } from '../lib/state.svelte';
import { translateMode } from '../lib/translate.svelte';
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
  translateMode.value = 'auto';
});

describe('PdfQuickActions translate toggle', () => {
  it('shows the 譯 toggle only when translate is enabled', async () => {
    appSettings.translate = { enabled: false };
    const { rerender } = render(PdfQuickActions, { props: { pill: makePill() } });
    expect(screen.queryByRole('button', { name: /translate/i })).not.toBeInTheDocument();

    appSettings.translate = { enabled: true, providers: ['llm'], default_provider: 'llm', target_lang: 'zh', trigger: 'auto' };
    await rerender({ pill: makePill() });
    expect(screen.getByRole('button', { name: /translate/i })).toBeInTheDocument();
  });

  it('switching to Manual persists via translateMode', async () => {
    appSettings.translate = { enabled: true, providers: ['llm'], default_provider: 'llm', target_lang: 'zh', trigger: 'auto' };
    render(PdfQuickActions, { props: { pill: makePill() } });
    await userEvent.click(screen.getByRole('button', { name: /translate/i }));
    await userEvent.click(screen.getByRole('button', { name: 'Manual' }));
    expect(translateMode.value).toBe('manual');
  });

  it('closes the mode popover on an outside click', async () => {
    appSettings.translate = { enabled: true, providers: ['llm'], default_provider: 'llm', target_lang: 'zh', trigger: 'auto' };
    render(PdfQuickActions, { props: { pill: makePill() } });
    await userEvent.click(screen.getByRole('button', { name: /translate/i }));
    expect(screen.getByRole('menu')).toBeInTheDocument();
    await userEvent.click(document.body);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});
