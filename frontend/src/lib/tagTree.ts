/// Tags nest by `/`-separated segments (e.g. `security/fuzzing`). These pure
/// helpers back both the pill-bar filter (Task 11) and chip highlighting
/// (Task 12): a filter on a parent tag matches its children too.

/// The first `/`-separated segment of a tag name, e.g. `security/fuzzing` -> `security`.
export const topLevel = (name: string): string => name.split('/', 1)[0];

/// Whether `tagName` is `filter` itself or one of its `filter/...` children.
export const isPrefixMatch = (tagName: string, filter: string): boolean =>
  tagName === filter || tagName.startsWith(filter + '/');
