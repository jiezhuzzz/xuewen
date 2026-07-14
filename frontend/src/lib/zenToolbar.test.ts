import { describe, expect, it } from 'vitest';
import { HIDE_DELAY_MS, HOT_ZONE_PX, holdVisible, toolbarVisible, type ToolbarHold } from './zenToolbar';

const none: ToolbarHold = {
  zen: true, hotZone: false, pointerOver: false,
  focusWithin: false, findOpen: false, pageEditing: false,
};

describe('holdVisible', () => {
  it('always holds outside zen', () => {
    expect(holdVisible({ ...none, zen: false })).toBe(true);
  });

  it('releases in zen once every hold drops', () => {
    expect(holdVisible(none)).toBe(false);
  });

  it.each([
    ['hotZone'], ['pointerOver'], ['focusWithin'], ['findOpen'], ['pageEditing'],
  ] as const)('%s alone holds the toolbar visible in zen', (k) => {
    expect(holdVisible({ ...none, [k]: true })).toBe(true);
  });
});

describe('toolbarVisible', () => {
  it('shows until the idle timer expires', () => {
    expect(toolbarVisible(none, false)).toBe(true);
    expect(toolbarVisible(none, true)).toBe(false);
  });

  it('an expired timer never hides a held toolbar', () => {
    expect(toolbarVisible({ ...none, findOpen: true }, true)).toBe(true);
    expect(toolbarVisible({ ...none, zen: false }, true)).toBe(true);
  });
});

it('exports sane constants', () => {
  expect(HIDE_DELAY_MS).toBeGreaterThan(0);
  expect(HOT_ZONE_PX).toBeGreaterThan(0);
});
