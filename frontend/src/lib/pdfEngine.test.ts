import { describe, expect, it } from 'vitest';
import { ENGINE_OPTIONS, themePreference, viewerPlugins } from './pdfEngine';

describe('themePreference', () => {
  it('returns the explicit mode when not system', () => {
    expect(themePreference('dark', false)).toBe('dark');
    expect(themePreference('light', true)).toBe('light');
  });
  it('resolves system mode from the OS flag', () => {
    expect(themePreference('system', true)).toBe('dark');
    expect(themePreference('system', false)).toBe('light');
  });
});

describe('ENGINE_OPTIONS', () => {
  it('is offline + main-thread (load-bearing)', () => {
    expect(ENGINE_OPTIONS.worker).toBe(false);
    expect(ENGINE_OPTIONS.wasmUrl).toBe('/pdfium.wasm');
    expect(ENGINE_OPTIONS.fontFallback).toBeNull();
  });
});

describe('viewerPlugins', () => {
  it('registers the document at the given href and includes the needed plugins', () => {
    const regs = viewerPlugins('/papers/abc/pdf');
    // Every registration exposes a package manifest with an id.
    const ids = regs.map((r) => r.package.manifest.id);
    for (const id of ['viewport', 'scroll', 'render', 'selection', 'interaction-manager', 'document-manager']) {
      expect(ids).toContain(id);
    }
    const docReg = regs.find((r) => r.package.manifest.id === 'document-manager');
    expect(docReg?.config?.initialDocuments?.[0]?.url).toBe('/papers/abc/pdf');
  });
});
