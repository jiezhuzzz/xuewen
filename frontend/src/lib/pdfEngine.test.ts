import { describe, expect, it } from 'vitest';
import { ENGINE_OPTIONS, viewerPlugins } from './pdfEngine';

describe('ENGINE_OPTIONS', () => {
  it('is offline + main-thread (load-bearing)', () => {
    expect(ENGINE_OPTIONS.worker).toBe(false);
    expect(ENGINE_OPTIONS.wasmUrl).toBe('/pdfium.wasm');
    expect(ENGINE_OPTIONS.fontFallback).toBeNull();
  });
});

describe('viewerPlugins', () => {
  it('includes the needed plugins and opens no document up front', () => {
    const regs = viewerPlugins();
    // Every registration exposes a package manifest with an id.
    const ids = regs.map((r) => r.package.manifest.id);
    for (const id of ['viewport', 'scroll', 'render', 'selection', 'interaction-manager', 'document-manager', 'tiling']) {
      expect(ids).toContain(id);
    }
    const docReg = regs.find((r) => r.package.manifest.id === 'document-manager');
    // Documents are opened per tab at runtime, not seeded here.
    expect(docReg?.config?.initialDocuments).toBeUndefined();
    // A cap high enough for many open tabs.
    expect(docReg?.config?.maxDocuments).toBeGreaterThanOrEqual(16);
  });
});
