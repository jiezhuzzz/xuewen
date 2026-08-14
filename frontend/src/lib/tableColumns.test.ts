import { describe, expect, it } from 'vitest';
import { PINNED_COLUMNS, PINNED_KEYS } from './tableColumns';

describe('PINNED_COLUMNS', () => {
  it('keeps min <= default <= max and a label for every column', () => {
    for (const key of PINNED_KEYS) {
      const def = PINNED_COLUMNS[key];
      expect(def.minWidth).toBeLessThanOrEqual(def.defaultWidth);
      expect(def.defaultWidth).toBeLessThanOrEqual(def.maxWidth);
      expect(def.label.length).toBeGreaterThan(0);
    }
  });
});
