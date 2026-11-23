// The narrow tier: windows under 1024 px wide (a 1280 window at 200 % zoom and more). The stage stacks in the page,
// which scrolls down, instead of floating its columns over the canvas (WCAG 1.4.10). The CSS writes the same width
// in its media queries; this is the script's side of it.
export const NARROW_QUERY = '(max-width: 1023px)';

export const isNarrow = (): boolean => globalThis.matchMedia?.(NARROW_QUERY).matches ?? false;
