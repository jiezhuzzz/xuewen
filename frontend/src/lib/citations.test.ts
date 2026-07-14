import { describe, expect, it } from 'vitest';
import { buildCitationData, findReferencesStart, type GotoLink, type PageText } from './citations';

function page(pageIndex: number, width: number, height: number, runs: PageText['runs']): PageText {
  return { pageIndex, width, height, runs, urlLinks: [] };
}

describe('findReferencesStart', () => {
  it('finds a standalone "References" heading and returns its position', () => {
    const pages = [
      page(0, 600, 800, [{ text: 'Introduction', x: 50, y: 100, width: 120, height: 14 }]),
      page(1, 600, 800, [
        { text: 'Conclusion', x: 50, y: 60, width: 100, height: 14 },
        { text: 'References', x: 50, y: 400, width: 90, height: 16 },
        { text: '[1] A. Author. A Title. Venue 2020.', x: 50, y: 430, width: 300, height: 12 },
      ]),
    ];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 1, y: 400, x: 50 });
  });

  it('also matches "Bibliography"', () => {
    const pages = [page(0, 600, 800, [{ text: 'Bibliography', x: 50, y: 200, width: 110, height: 16 }])];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 0, y: 200, x: 50 });
  });

  it('detects a heading split across runs on one line (small-caps / drop cap)', () => {
    // Real PDFs render "REFERENCES" with a large first letter, which PDFium
    // splits into separate runs on the same baseline (same y). The line must be
    // reconstructed from its runs before matching.
    const pages = [
      page(5, 600, 800, [
        { text: 'R', x: 50, y: 300, width: 14, height: 16 },
        { text: 'EFERENCES', x: 64, y: 300, width: 80, height: 12 },
        { text: '[1] A. Author. Title. 2020.', x: 50, y: 330, width: 300, height: 12 },
      ]),
    ];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 5, y: 300, x: 50 });
  });

  it('matches numbered headings: "7 References"', () => {
    const pages = [page(0, 600, 800, [{ text: '7 References', x: 50, y: 200, width: 120, height: 16 }])];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 0, y: 200, x: 50 });
  });

  it('matches roman-numbered headings: "VII. References"', () => {
    const pages = [page(0, 600, 800, [{ text: 'VII. References', x: 50, y: 200, width: 140, height: 16 }])];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 0, y: 200, x: 50 });
  });

  it('matches "References and Notes" and "Works Cited"', () => {
    const a = [page(0, 600, 800, [{ text: 'References and Notes', x: 50, y: 100, width: 180, height: 16 }])];
    const b = [page(0, 600, 800, [{ text: 'Works Cited', x: 50, y: 100, width: 100, height: 16 }])];
    expect(findReferencesStart(a)).toEqual({ pageIndex: 0, y: 100, x: 50 });
    expect(findReferencesStart(b)).toEqual({ pageIndex: 0, y: 100, x: 50 });
  });

  it('matches "References Cited"', () => {
    const pages = [page(0, 600, 800, [{ text: 'References Cited', x: 50, y: 100, width: 140, height: 16 }])];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 0, y: 100, x: 50 });
  });

  it('does not match ordinary words spelled from roman letters', () => {
    const pages = [
      page(0, 600, 800, [
        { text: 'Mild References', x: 50, y: 100, width: 140, height: 16 },
        { text: 'Civil References', x: 50, y: 130, width: 140, height: 16 },
        { text: 'D References', x: 50, y: 160, width: 120, height: 16 },
      ]),
    ];
    expect(findReferencesStart(pages)).toBeNull();
  });

  it('does not match "I. Introduction" or the word inside a sentence', () => {
    const pages = [
      page(0, 600, 800, [
        { text: 'I. Introduction', x: 50, y: 100, width: 140, height: 16 },
        { text: 'as listed in the references below', x: 50, y: 130, width: 260, height: 12 },
        { text: 'preferences', x: 50, y: 160, width: 100, height: 12 },
      ]),
    ];
    expect(findReferencesStart(pages)).toBeNull();
  });

  it('ignores the word inside a sentence (not a heading)', () => {
    const pages = [page(0, 600, 800, [
      { text: 'see the references section for details', x: 50, y: 100, width: 260, height: 12 },
    ])];
    expect(findReferencesStart(pages)).toBeNull();
  });

  it('returns null when there is no references section', () => {
    const pages = [page(0, 600, 800, [{ text: 'Just body text here', x: 50, y: 100, width: 150, height: 12 }])];
    expect(findReferencesStart(pages)).toBeNull();
  });
});

describe('buildCitationData', () => {
  // Two references on page 1 at y=430 and y=470; two markers on page 0 that
  // point at them. A third link points ABOVE the references start (a figure
  // link) and must be ignored.
  const pages: PageText[] = [
    page(0, 600, 800, [{ text: 'body [1] and [2]', x: 50, y: 100, width: 140, height: 12 }]),
    { pageIndex: 1, width: 600, height: 800, urlLinks: [
        { x: 300, y: 432, width: 80, height: 12, url: 'https://doi.org/10.1/adam' },
      ],
      runs: [
        { text: 'References', x: 50, y: 400, width: 90, height: 16 },
        { text: '[1] Kingma, Ba. Adam. ICLR 2015.', x: 50, y: 430, width: 320, height: 12 },
        { text: '[2] He et al. ResNet. CVPR 2016.', x: 50, y: 470, width: 320, height: 12 },
      ] },
  ];
  const refStart: import('./citations').RefAnchor = { pageIndex: 1, y: 400, x: 50 };
  const links: GotoLink[] = [
    { pageIndex: 0, x: 90, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 430, destX: 0 },
    { pageIndex: 0, x: 120, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 470, destX: 0 },
    { pageIndex: 0, x: 200, y: 300, width: 12, height: 12, destPageIndex: 1, destY: 150, destX: 0 }, // above refStart → ignored
  ];

  it('orders references by destination and extracts their raw text', () => {
    const { references } = buildCitationData(links, pages, refStart);
    expect(references.map((r) => r.index)).toEqual([0, 1]);
    expect(references[0].rawText).toBe('[1] Kingma, Ba. Adam. ICLR 2015.');
    expect(references[1].rawText).toBe('[2] He et al. ResNet. CVPR 2016.');
  });

  it('captures an external URL inside a reference entry', () => {
    const { references } = buildCitationData(links, pages, refStart);
    expect(references[0].externalUrl).toBe('https://doi.org/10.1/adam');
    expect(references[1].externalUrl).toBeUndefined();
  });

  it('maps each in-references marker to its reference index and drops out-of-region links', () => {
    const { markers } = buildCitationData(links, pages, refStart);
    expect(markers).toEqual([
      { pageIndex: 0, x: 90, y: 100, width: 12, height: 12, refIndex: 0 },
      { pageIndex: 0, x: 120, y: 100, width: 12, height: 12, refIndex: 1 },
    ]);
  });

  it('dedupes markers that share a destination into one reference', () => {
    // Two markers point at the SAME reference (destY 430 and 433 are within
    // DEST_EPSILON=6), a third points at a distinct reference (470). The near-equal
    // destinations must collapse to one reference; both markers map to it.
    const dupLinks: GotoLink[] = [
      { pageIndex: 0, x: 90, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 430, destX: 0 },
      { pageIndex: 0, x: 140, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 433, destX: 0 },
      { pageIndex: 0, x: 200, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 470, destX: 0 },
    ];
    const { references, markers } = buildCitationData(dupLinks, pages, refStart);
    expect(references).toHaveLength(2);
    expect(markers.map((m) => m.refIndex)).toEqual([0, 0, 1]);
  });
});

describe('two-column bibliographies', () => {
  // 600pt-wide page, mid=300: left column x≈50, right column x≈320.
  // Left column: [1] at y=100 (its text continues into the right column top),
  // Right column: [2] at y=100 — same y as [1] but a DIFFERENT reference.
  const p = page(3, 600, 800, [
    { text: 'References', x: 50, y: 60, width: 90, height: 16 },
    { text: '[1] A. Adam paper line one', x: 50, y: 100, width: 220, height: 12 },
    { text: 'continued in left column', x: 50, y: 120, width: 200, height: 12 },
    { text: 'and finishes atop the right column.', x: 320, y: 60, width: 220, height: 12 },
    { text: '[2] B. Bert paper.', x: 320, y: 100, width: 200, height: 12 },
  ]);
  const links: GotoLink[] = [
    { pageIndex: 0, x: 90, y: 700, width: 12, height: 12, destPageIndex: 3, destY: 100, destX: 50 },
    { pageIndex: 0, x: 120, y: 700, width: 12, height: 12, destPageIndex: 3, destY: 100, destX: 320 },
  ];
  const refStart = { pageIndex: 3, y: 60, x: 50 };

  it('keeps same-y anchors in different columns as distinct references', () => {
    const data = buildCitationData(links, [p], refStart);
    expect(data.references).toHaveLength(2);
  });

  it('slices entry text column-major so an entry flows across the column break', () => {
    const data = buildCitationData(links, [p], refStart);
    const first = data.references.find((r) => r.rawText.startsWith('[1]'))!;
    expect(first.rawText).toBe('[1] A. Adam paper line one continued in left column and finishes atop the right column.');
    const second = data.references.find((r) => r.rawText.startsWith('[2]'))!;
    expect(second.rawText).toBe('[2] B. Bert paper.');
  });

  it('includes right-column entries ABOVE the heading y on the heading page', () => {
    // heading in the LEFT column at y=60; a right-column entry at y=30 is
    // still "after" it in column-major (reading) order.
    const p2 = page(3, 600, 800, [
      { text: 'References', x: 50, y: 60, width: 90, height: 16 },
      { text: '[1] Left entry.', x: 50, y: 100, width: 150, height: 12 },
      { text: '[2] Right entry above heading y.', x: 320, y: 30, width: 220, height: 12 },
    ]);
    const links2: GotoLink[] = [
      { pageIndex: 0, x: 90, y: 700, width: 12, height: 12, destPageIndex: 3, destY: 100, destX: 50 },
      { pageIndex: 0, x: 120, y: 700, width: 12, height: 12, destPageIndex: 3, destY: 30, destX: 320 },
    ];
    const data = buildCitationData(links2, [p2], { pageIndex: 3, y: 60, x: 50 });
    expect(data.references).toHaveLength(2);
  });
});
