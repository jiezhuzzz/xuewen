/// Placement for a popover anchored to a screen point (citation hover,
/// translate selection). It normally sits ABOVE the anchor (the caller pairs
/// `below: false` with `translateY(-100%)`); near the top of the viewport
/// that would clip off-screen (looked like "no popup"), so flip it BELOW.
/// Also clamp horizontally so it never runs off the right edge.
const MARGIN = 8;

export function anchoredPosition(
  x: number,
  y: number,
  opts: { maxW: number; belowOffset: number },
): { left: number; top: number; below: boolean } {
  const vw = typeof window === 'undefined' ? 1280 : window.innerWidth;
  const below = y < 220;
  return {
    left: Math.max(MARGIN, Math.min(x, vw - opts.maxW - MARGIN)),
    top: below ? y + opts.belowOffset : y - MARGIN,
    below,
  };
}
