import { describe, expect, it } from 'vitest';
import { abbreviateVenue } from './venue';

describe('abbreviateVenue', () => {
  it('maps a full name to its canonical abbreviation', () => {
    expect(abbreviateVenue('2025 IEEE Symposium on Security and Privacy (SP)')).toBe('S&P');
  });

  it('canonicalizes a bare acronym found in the string (S&P)', () => {
    expect(abbreviateVenue('IEEE S&P')).toBe('S&P');
  });

  it('maps Neural Information Processing Systems to NeurIPS', () => {
    expect(abbreviateVenue('Advances in Neural Information Processing Systems')).toBe('NeurIPS');
  });

  it('recognizes an already-short acronym via the curated map (ICML)', () => {
    expect(abbreviateVenue('ICML')).toBe('ICML');
  });

  it('canonicalizes a messy real acronym (NAACL-HLT → NAACL)', () => {
    expect(abbreviateVenue('NAACL-HLT')).toBe('NAACL');
  });

  it('falls back to a trailing parenthetical acronym when unmapped', () => {
    expect(abbreviateVenue('2024 Conference on Made Up Things (CMUT)')).toBe('CMUT');
  });

  it('strips a leading year when unmapped and has no parenthetical', () => {
    expect(abbreviateVenue('2019 Journal of Obscure Studies')).toBe('Journal of Obscure Studies');
  });

  it('passes an unlisted bare acronym through unchanged', () => {
    expect(abbreviateVenue('FLARB')).toBe('FLARB');
  });

  it('returns null and empty unchanged', () => {
    expect(abbreviateVenue(null)).toBeNull();
    expect(abbreviateVenue('')).toBe('');
  });
});
