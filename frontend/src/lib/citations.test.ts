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
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 1, y: 400 });
  });

  it('also matches "Bibliography"', () => {
    const pages = [page(0, 600, 800, [{ text: 'Bibliography', x: 50, y: 200, width: 110, height: 16 }])];
    expect(findReferencesStart(pages)).toEqual({ pageIndex: 0, y: 200 });
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
  const refStart: import('./citations').RefAnchor = { pageIndex: 1, y: 400 };
  const links: GotoLink[] = [
    { pageIndex: 0, x: 90, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 430 },
    { pageIndex: 0, x: 120, y: 100, width: 12, height: 12, destPageIndex: 1, destY: 470 },
    { pageIndex: 0, x: 200, y: 300, width: 12, height: 12, destPageIndex: 1, destY: 150 }, // above refStart → ignored
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
});
