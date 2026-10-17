/** In-app history depth (plans/m11-profiles.md, P1 apps v2 6). The router
 * stamps every entry with `_depth` (history.length - 1 at first load), which
 * counts pages from before the app. Only entries above the base are ours to go
 * back through; a profile switch moves the base so Back never returns to the
 * old profile's pages. */

export const historyDepth = () => (history.state as { _depth?: number } | null)?._depth ?? 0;

let base = historyDepth();
export const historyBase = () => base;

/** After a profile switch: the entries below this one belong to the old profile. */
export function resetHistoryBase() {
  base = historyDepth();
}
