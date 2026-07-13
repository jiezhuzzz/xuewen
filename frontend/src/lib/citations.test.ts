import { describe, expect, it } from 'vitest';
import { findReferencesStart, type PageText } from './citations';

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
