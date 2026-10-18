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
// Only a reload or a return through history is the same visit; a page loaded
// anew starts its own base.
const sameVisit = (() => {
  const nav = performance.getEntriesByType?.("navigation")[0] as PerformanceNavigationTiming | undefined;
  return nav?.type === "reload" || nav?.type === "back_forward";
})();
let base = stored === null || !sameVisit ? historyDepth() : Number(stored);
export const historyBase = () => base;

/** After a profile switch: the entries below [depth] (this one by default) belong to the old profile. */
export function resetHistoryBase(depth = historyDepth()) {
  base = depth;
  try {
    sessionStorage.setItem(BASE, String(base));
  } catch {
    // Kept for this page only.
  }
}
