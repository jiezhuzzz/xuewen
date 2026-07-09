import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { initTheme, theme, toggleTheme } from './state.svelte';

// Control the OS preference the code reads via matchMedia, and capture the
// 'change' listener so we can simulate the OS flipping at runtime.
let prefersDark = false;
let changeListener: (() => void) | null = null;

function mockMatchMedia(): void {
  changeListener = null;
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: prefersDark,
    media: query,
    addEventListener: (_type: string, cb: () => void) => {
      changeListener = cb;
    },
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }));
}

const isDark = () => document.documentElement.classList.contains('dark');

beforeEach(() => {
  prefersDark = false;
  localStorage.clear();
  document.documentElement.classList.remove('dark');
  theme.mode = 'system';
  mockMatchMedia();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('theme', () => {
  it('defaults to system when nothing is saved', () => {
    initTheme();
    expect(theme.mode).toBe('system');
  });

  it('restores a saved mode', () => {
    localStorage.setItem('xuewen-theme', 'dark');
    initTheme();
    expect(theme.mode).toBe('dark');
    expect(isDark()).toBe(true);
  });

  it('falls back to system for an unknown saved value', () => {
    localStorage.setItem('xuewen-theme', 'bogus');
    initTheme();
    expect(theme.mode).toBe('system');
  });

  it('system mode follows the OS preference', () => {
    prefersDark = true;
    initTheme();
    expect(theme.mode).toBe('system');
    expect(isDark()).toBe(true);

    prefersDark = false;
    changeListener?.();
    expect(isDark()).toBe(false);
  });

  it('explicit modes ignore the OS preference', () => {
    prefersDark = true;
    localStorage.setItem('xuewen-theme', 'light');
    initTheme();
    expect(isDark()).toBe(false);

    // OS flip should not move an explicitly-light theme.
    changeListener?.();
    expect(isDark()).toBe(false);
  });

  it('cycles light -> dark -> system and persists', () => {
    localStorage.setItem('xuewen-theme', 'light');
    initTheme();
    expect(theme.mode).toBe('light');

    toggleTheme();
    expect(theme.mode).toBe('dark');
    expect(localStorage.getItem('xuewen-theme')).toBe('dark');

    toggleTheme();
    expect(theme.mode).toBe('system');
    expect(localStorage.getItem('xuewen-theme')).toBe('system');

    toggleTheme();
    expect(theme.mode).toBe('light');
  });
});
