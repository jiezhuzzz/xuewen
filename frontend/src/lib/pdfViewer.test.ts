import { describe, it, expect } from 'vitest';
import { pdfViewerConfig, themePreference } from './pdfViewer';

describe('themePreference', () => {
  it('passes explicit modes through', () => {
    expect(themePreference('light', true)).toBe('light');
    expect(themePreference('dark', false)).toBe('dark');
  });
  it('resolves system to the OS preference', () => {
    expect(themePreference('system', true)).toBe('dark');
    expect(themePreference('system', false)).toBe('light');
  });
});

describe('pdfViewerConfig', () => {
  it('builds an offline, self-hosted config for a paper', () => {
    const c = pdfViewerConfig('p1', 'dark');
    expect(c.src).toBe('/papers/p1/pdf');
    expect(c.wasmUrl).toBe('/pdfium.wasm');
    expect(c.worker).toBe(false);
    expect(c.fonts).toEqual({ ui: null, signature: null });
    expect(c.fontFallback).toBeNull();
    expect(c.stamp).toEqual({ manifests: [] });
    expect(c.tabBar).toBe('never');
    const theme = c.theme as {
      preference: string;
      light: { accent: { primary: string }; background: { surface: string } };
      dark: { accent: { primary: string }; background: { surface: string } };
    };
    expect(theme.preference).toBe('dark');
    // Palette aligns the viewer chrome with the web UI: amber accent + warm surfaces.
    expect(theme.light.accent.primary).toBe('#b45309'); // amber-700
    expect(theme.dark.accent.primary).toBe('#f59e0b'); // amber-500
    expect(theme.light.background.surface).toBe('#faf9f7'); // paper
    expect(theme.dark.background.surface).toBe('#211d1a'); // soot
  });

  it('percent-encodes the paper id in src', () => {
    expect(pdfViewerConfig('a b/c%d', 'light').src).toBe('/papers/a%20b%2Fc%25d/pdf');
  });
});
