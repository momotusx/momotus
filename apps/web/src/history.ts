/** In-app history depth (plans/m11-profiles.md, P1 apps v2 6). The router
 * stamps every entry with `_depth` (history.length - 1 at first load), which
 * counts pages from before the app. Only entries above the base are ours to go
 * back through; a profile switch moves the base so Back never returns to the
 * old profile's pages. */

export const historyDepth = () => (history.state as { _depth?: number } | null)?._depth ?? 0;

// Kept for the tab, so a reload does not reopen the old profile's entries.
const BASE = "mq.historyBase";
const stored = (() => {
  try {
    return sessionStorage.getItem(BASE);
  } catch {
    return null;
  }
})();
let base = stored === null ? historyDepth() : Number(stored);
export const historyBase = () => base;

/** After a profile switch: the entries below this one belong to the old profile. */
export function resetHistoryBase() {
  base = historyDepth();
  try {
    sessionStorage.setItem(BASE, String(base));
  } catch {
    // Kept for this page only.
  }
}
