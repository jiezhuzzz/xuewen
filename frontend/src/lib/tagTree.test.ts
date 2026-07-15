import { describe, expect, test } from 'vitest';
import { isPrefixMatch, topLevel } from './tagTree';

describe('isPrefixMatch', () => {
  test('prefix match includes children', () => {
    expect(isPrefixMatch('security', 'security')).toBe(true);
    expect(isPrefixMatch('security/fuzzing', 'security')).toBe(true);
    expect(isPrefixMatch('ml/llm', 'security')).toBe(false);
  });
});

describe('topLevel', () => {
  test('topLevel is the first segment', () => {
    expect(topLevel('security/fuzzing')).toBe('security');
    expect(topLevel('benchmarks')).toBe('benchmarks');
  });
});
