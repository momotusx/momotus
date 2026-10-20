/** In-app history (plans/m11-profiles.md, P1 apps v2 6). The router stamps
 * every entry with `_depth` (history.length - 1 at first load), which counts
 * pages from before the app; the app's own Back goes only through entries
 * above the base. Every entry also carries the profile it was opened under
 * (`_profile`), written by the history calls themselves, so the stamp always
 * lands on the entry the router actually wrote (the router changes the route
 * before it writes history). */

import { createSignal } from "solid-js";

// Back (the app's own or the browser's) is under way: the screen it lands on
// restores the viewer's focus instead of placing its own first focus.
let wentBack = false;
export const markBack = () => {
  wentBack = true;
};
/** While the screen Back lands on is created: whether it was reached by Back. */
export const cameBack = () => wentBack;
/** The shell, once the new screen is in: reads and clears the mark. */
export function takeBack(): boolean {
  const was = wentBack;
  wentBack = false;
  return was;
}

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
// A fresh visit's base is kept too, so its own reload finds it.
try {
  sessionStorage.setItem(BASE, String(base));
} catch {
  // Kept for this page only.
}

const stampOf = () => (history.state as { _profile?: string } | null)?._profile;
const [entryOwner, setEntryOwner] = createSignal(stampOf());
/** The profile the current history entry was opened under; undefined before the app knew one. */
export { entryOwner };

let shown: () => string | undefined = () => undefined;
let open: () => boolean = () => false;
/** Tells the history calls which profile the session runs (every entry written from now carries
 * it) and whether its content shows. */
export function stampEntriesWith(session: () => string | undefined, gateOpen: () => boolean) {
  shown = session;
  open = gateOpen;
}

for (const name of ["pushState", "replaceState"] as const) {
  const native = history[name].bind(history);
  history[name] = (state: unknown, unused: string, url?: string | URL | null) => {
    const fields = (state ?? {}) as Record<string, unknown>;
    const owner = shown() ?? (fields._profile as string | undefined);
    // The page this entry was opened from (`_from`): a push records the page
    // being left; a replace keeps what the entry had, since Back still goes there.
    const from = name === "pushState" ? location.pathname : (fields._from ?? entryFrom());
    native({ ...fields, _profile: owner, _from: from }, unused, url);
    setEntryOwner(owner);
  };
}

/** The path of the page this history entry was opened from in the app, or null. */
export function entryFrom(): string | null {
  const from = (history.state as { _from?: unknown } | null)?._from;
  return typeof from === "string" ? from : null;
}
// Registered before the router's own listener, so it runs first: an entry of
// another profile reached with the browser's Back or Forward becomes Home
// before the router reads the address, and the app's Back stops there.
window.addEventListener("popstate", () => {
  const owner = stampOf();
  const active = shown();
  if (open() && owner && active && owner !== active) {
    history.replaceState(history.state, "", `/${window.location.search}`);
    base = historyDepth();
    try {
      sessionStorage.setItem(BASE, String(base));
    } catch {
      // Kept for this page only.
    }
  }
  setEntryOwner(stampOf());
});

/** After a profile switch: the entries below [depth] (this one by default) belong to the old profile. */
export function resetHistoryBase(depth = historyDepth()) {
  base = depth;
  try {
    sessionStorage.setItem(BASE, String(base));
  } catch {
    // Kept for this page only.
  }
}
