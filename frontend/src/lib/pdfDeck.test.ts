import { describe, expect, it } from 'vitest';
import { reconcileDocuments } from './pdfDeck';

describe('reconcileDocuments', () => {
  it('opens new tabs and closes removed ones', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], ['b', 'c']);
    expect(toOpen).toEqual(['c']);
    expect(toClose).toEqual(['a']);
  });

  it('is a no-op when opened matches the tabs exactly', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], ['a', 'b']);
    expect(toOpen).toEqual([]);
    expect(toClose).toEqual([]);
  });

  it('opens everything when nothing is opened yet', () => {
    const { toOpen, toClose } = reconcileDocuments([], ['a', 'b', 'c']);
    expect(toOpen).toEqual(['a', 'b', 'c']);
    expect(toClose).toEqual([]);
  });

  it('closes everything when there are no tabs left', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], []);
    expect(toOpen).toEqual([]);
    expect(toClose).toEqual(['a', 'b']);
  });
});
