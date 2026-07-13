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
    expect(c.worker).toBe(true);
    expect(c.fonts).toEqual({ ui: null, signature: null });
    expect(c.fontFallback).toBeNull();
    expect(c.stamp).toEqual({ manifests: [] });
    expect(c.tabBar).toBe('never');
    expect(c.theme).toEqual({ preference: 'dark' });
  });
});
