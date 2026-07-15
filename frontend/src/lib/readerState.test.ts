import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { dropReaderState, openFind, reader, setFind, setPanelView, toggleSidebar } from './readerState.svelte';

beforeEach(() => {
  for (const k of Object.keys(reader.find)) delete reader.find[k];
  reader.panel = null;
  reader.lastPanel = 'thumbs';
});

describe('setFind', () => {
  it('toggles when called without an explicit state', () => {
    setFind('a');
    expect(reader.find['a']).toBe(true);
    setFind('a');
    expect(reader.find['a']).toBe(false);
  });

  it('forces the given state', () => {
    setFind('a', true);
    setFind('a', true);
    expect(reader.find['a']).toBe(true);
    setFind('a', false);
    expect(reader.find['a']).toBe(false);
  });
});

describe('toggleSidebar / setPanelView (global)', () => {
  it('opens at thumbnails first, closes on re-toggle', () => {
    toggleSidebar();
    expect(reader.panel).toBe('thumbs');
    toggleSidebar();
    expect(reader.panel).toBe(null);
  });

  it('reopens at the last-used view', () => {
    toggleSidebar();
    setPanelView('outline');
    toggleSidebar(); // close
    expect(reader.panel).toBe(null);
    toggleSidebar(); // reopen
    expect(reader.panel).toBe('outline');
  });

  it('is a single global setting, not keyed per document', () => {
    toggleSidebar();
    setPanelView('outline');
    // One shared value applies to every open paper.
    expect(reader.panel).toBe('outline');
  });
});

describe('openFind', () => {
  it('opens the bar and focuses that document’s find input', async () => {
    const input = document.createElement('input');
    input.setAttribute('data-find-input', 'a');
    document.body.appendChild(input);
    openFind('a');
    expect(reader.find['a']).toBe(true);
    await tick(); // tick
    await tick(); // Allow callback to complete
    expect(document.activeElement).toBe(input);
    input.remove();
  });
});

describe('dropReaderState', () => {
  it('forgets a closed document’s find state but leaves the global panel', () => {
    setFind('a', true);
    toggleSidebar();
    setPanelView('outline');
    dropReaderState('a');
    expect(reader.find['a']).toBeUndefined();
    expect(reader.panel).toBe('outline'); // global — unaffected by closing a tab
  });
});
