import { describe, expect, it } from 'vitest';
import { ENGINE_OPTIONS, viewerPlugins } from './pdfEngine';

describe('ENGINE_OPTIONS', () => {
  it('is offline + runs PDFium in a worker (load-bearing)', () => {
    expect(ENGINE_OPTIONS.worker).toBe(true);
    // Resolved to a fully-qualified same-origin URL (not a bare path) — the
    // stock blob worker's self.location is a blob: URL, which can't resolve
    // a path-absolute fetch like '/pdfium.wasm' against it. See pdfEngine.ts.
    expect(ENGINE_OPTIONS.wasmUrl).toBe(new URL('/pdfium.wasm', location.origin).href);
    expect(ENGINE_OPTIONS.wasmUrl.endsWith('/pdfium.wasm')).toBe(true);
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
