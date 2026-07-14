import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { dropReaderState, openFind, reader, setFind, togglePanel } from './readerState.svelte';

beforeEach(() => {
  for (const k of Object.keys(reader.find)) delete reader.find[k];
  for (const k of Object.keys(reader.panel)) delete reader.panel[k];
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

describe('togglePanel', () => {
  it('opens a tab, switches tabs, and closes on re-select', () => {
    togglePanel('a', 'thumbs');
    expect(reader.panel['a']).toBe('thumbs');
    togglePanel('a', 'outline');
    expect(reader.panel['a']).toBe('outline');
    togglePanel('a', 'outline');
    expect(reader.panel['a']).toBe(null);
  });

  it('keeps state independent per document', () => {
    togglePanel('a', 'thumbs');
    togglePanel('b', 'outline');
    expect(reader.panel['a']).toBe('thumbs');
    expect(reader.panel['b']).toBe('outline');
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
  it('forgets a closed document', () => {
    setFind('a', true);
    togglePanel('a', 'thumbs');
    dropReaderState('a');
    expect(reader.find['a']).toBeUndefined();
    expect(reader.panel['a']).toBeUndefined();
  });
});
