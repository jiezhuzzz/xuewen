import { beforeEach, describe, expect, it } from 'vitest';
import { initInfo, setInfoOpen, toggleInfo, viewer } from './state.svelte';

beforeEach(() => {
  viewer.infoOpen = false;
  localStorage.clear();
});

describe('info-panel persistence', () => {
  it('setInfoOpen updates state and localStorage', () => {
    setInfoOpen(true);
    expect(viewer.infoOpen).toBe(true);
    expect(localStorage.getItem('xuewen-info-open')).toBe('1');
    setInfoOpen(false);
    expect(localStorage.getItem('xuewen-info-open')).toBe('0');
  });

  it('toggleInfo flips the current value', () => {
    viewer.infoOpen = false;
    toggleInfo();
    expect(viewer.infoOpen).toBe(true);
    toggleInfo();
    expect(viewer.infoOpen).toBe(false);
  });

  it('initInfo restores the remembered value (default closed)', () => {
    initInfo();
    expect(viewer.infoOpen).toBe(false); // nothing stored yet
    localStorage.setItem('xuewen-info-open', '1');
    initInfo();
    expect(viewer.infoOpen).toBe(true);
  });
});
