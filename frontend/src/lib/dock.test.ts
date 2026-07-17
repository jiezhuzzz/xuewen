import { beforeEach, describe, expect, it } from 'vitest';
import { closeDock, dock, initDock, openDock, toggleDock, viewer } from './state.svelte';

beforeEach(() => {
  localStorage.clear();
  dock.open = false;
  dock.tab = 'details';
  viewer.activeId = 'p1';
});

describe('dock state', () => {
  it('toggleDock opens on the requested tab', () => {
    toggleDock('ask');
    expect(dock.open).toBe(true);
    expect(dock.tab).toBe('ask');
  });

  it('re-toggling the open tab closes; the other tab switches', () => {
    openDock('details');
    toggleDock('ask'); // switch, stay open
    expect(dock.open).toBe(true);
    expect(dock.tab).toBe('ask');
    toggleDock('ask'); // same tab -> close
    expect(dock.open).toBe(false);
  });

  it('toggleDock is a no-op without an active PDF tab', () => {
    viewer.activeId = null;
    toggleDock('details');
    expect(dock.open).toBe(false);
  });

  it('open/close persist and initDock restores them', () => {
    openDock('ask');
    dock.open = false;
    dock.tab = 'details';
    initDock();
    expect(dock.open).toBe(true);
    expect(dock.tab).toBe('ask');
    closeDock();
    dock.open = true;
    initDock();
    expect(dock.open).toBe(false);
  });

  it('initDock tolerates corrupted storage', () => {
    localStorage.setItem('xuewen-dock', '{nope');
    initDock();
    expect(dock.open).toBe(false);
    expect(dock.tab).toBe('details');
  });
});
