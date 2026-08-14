/// The one style for a paper's manual "known as" name (`papers.name`) wherever
/// it appears as a chip: the sidebar row and the Details dock.
///
/// Amber because the name is how you actually refer to the paper — "the RVSpec
/// paper" — so it has to survive a glance down a dense list; the muted stone it
/// used to wear made it read as fine print next to the search-match field label
/// it happened to match.
///
/// Mono is what keeps it apart from the tag chips, which are the same amber
/// whenever a tag filter is active (`chipClasses` in PaperRowTags.svelte).
/// Identity and topic can't be told apart by color alone, so: mono = the thing
/// the paper proposes, sans = what it's about.
///
/// Deliberately no `display` utility. Applied to an inline <span> inside the
/// sidebar's line-clamp-2 title, padding and borders on an inline box don't
/// grow the line box, so the chip can never push a row taller; `inline-block`
/// (which the design mock used) would.
export const NAME_CHIP =
  'rounded border border-amber-700/40 bg-amber-700/10 px-1.5 py-px font-mono text-chip font-semibold text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/15 dark:text-amber-400';
