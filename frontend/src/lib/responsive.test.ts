import { beforeEach, describe, expect, it, vi } from 'vitest';
import { initResponsiveSidebar, ui } from './state.svelte';

// Control the viewport width the code reads via matchMedia, and capture the
// 'change' listener so we can simulate window resizes across the breakpoint.
let narrow = false;
let changeListener: ((e: { matches: boolean }) => void) | null = null;

function mockMatchMedia(): void {
  changeListener = null;
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: narrow,
    media: query,
    addEventListener: (_type: string, cb: (e: { matches: boolean }) => void) => {
      changeListener = cb;
    },
    removeEventListener: () => {},
  }));
}

beforeEach(() => {
  ui.sidebarOpen = true;
  ui.zen = false;
  mockMatchMedia();
});

describe('responsive sidebar', () => {
  it('collapses the sidebar when starting below the breakpoint', () => {
    narrow = true;
    initResponsiveSidebar();
    expect(ui.sidebarOpen).toBe(false);
  });

  it('leaves the sidebar open when starting wide', () => {
    narrow = false;
    initResponsiveSidebar();
    expect(ui.sidebarOpen).toBe(true);
  });

  it('follows live crossings of the breakpoint in both directions', () => {
    narrow = false;
    initResponsiveSidebar();
    changeListener!({ matches: true });
    expect(ui.sidebarOpen).toBe(false);
    changeListener!({ matches: false });
    expect(ui.sidebarOpen).toBe(true);
  });
});
