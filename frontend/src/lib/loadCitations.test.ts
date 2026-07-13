import { PdfActionType, PdfZoomMode } from '@embedpdf/models';
import { describe, expect, it } from 'vitest';
import { loadCitations, type EngineLike } from './loadCitations';

function task<T>(v: T) { return { toPromise: () => Promise.resolve(v) }; }

// Minimal fake doc: 2 pages, 800pt tall. EmbedPDF returns annotation and text
// rects in top-left device space (given here directly: heading y=400, reference
// entry y=430), but GoTo destination `y` in PDF bottom-left space — so the
// marker's destination y is 370 (= 800 − 430), which loadCitations flips back to
// 430 to line it up with the reference entry.
const doc: any = {
  id: 'd', pageCount: 2,
  pages: [
    { index: 0, size: { width: 600, height: 800 }, rotation: 0 },
    { index: 1, size: { width: 600, height: 800 }, rotation: 0 },
  ],
};

const engine: EngineLike = {
  getPageAnnotations: (_d, page: any) => task(
    page.index === 0
      ? [
          // a LINK marker on page 0 whose GoTo destination (bottom-left y=370)
          // flips to the reference entry (page 1, top-left y=430).
          { type: 2 /* LINK */, pageIndex: 0, rect: { origin: { x: 90, y: 100 }, size: { width: 12, height: 12 } },
            target: {
              type: 'destination',
              destination: { pageIndex: 1, view: [], zoom: { mode: PdfZoomMode.XYZ, params: { x: 50, y: 370, zoom: 0 } } },
            } },
        ]
      : [
          // a URI link inside the reference entry on page 1 (y=432, in its band)
          { type: 2 /* LINK */, pageIndex: 1, rect: { origin: { x: 300, y: 432 }, size: { width: 80, height: 12 } },
            target: { type: 'action', action: { type: PdfActionType.URI, uri: 'https://doi.org/10.1/adam' } } },
        ],
  ),
  getPageTextRuns: (_d, page: any) => task(
    page.index === 1
      ? { runs: [
          { text: 'References', rect: { origin: { x: 50, y: 400 }, size: { width: 90, height: 16 } } },
          { text: '[1] Kingma, Ba. Adam. ICLR 2015.', rect: { origin: { x: 50, y: 430 }, size: { width: 320, height: 12 } } },
        ] }
      : { runs: [{ text: 'body [1]', rect: { origin: { x: 50, y: 100 }, size: { width: 80, height: 12 } } }] },
  ),
};

describe('loadCitations', () => {
  it('produces one reference and one marker from engine annotations + text', async () => {
    const { references, markers } = await loadCitations(engine, doc);
    expect(references).toHaveLength(1);
    expect(references[0].rawText).toContain('Adam');
    expect(references[0].externalUrl).toBe('https://doi.org/10.1/adam');
    expect(markers).toHaveLength(1);
    expect(markers[0]).toMatchObject({ pageIndex: 0, refIndex: 0 });
  });

  it('returns empty when there is no references heading', async () => {
    const noRefEngine: EngineLike = {
      getPageAnnotations: engine.getPageAnnotations,
      getPageTextRuns: (_d, _p) => task({ runs: [{ text: 'body', rect: { origin: { x: 0, y: 0 }, size: { width: 1, height: 1 } } }] }),
    };
    const { references, markers } = await loadCitations(noRefEngine, doc);
    expect(references).toHaveLength(0);
    expect(markers).toHaveLength(0);
  });
});
