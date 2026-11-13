/** In-app history (plans/m11-profiles.md, P1 apps v2 6). The router stamps
 * every entry with `_depth` (history.length - 1 at first load), which counts
 * pages from before the app; the app's own Back goes only through entries
 * above the base. Every entry also carries the profile it was opened under
 * (`_profile`), written by the history calls themselves, so the stamp always
 * lands on the entry the router actually wrote (the router changes the route
 * before it writes history). */

import { createSignal } from "solid-js";
import { screenChanged } from "./focus/ghost-tap";

// Back (the app's own or the browser's) is under way: the screen it lands on
// restores the viewer's focus instead of placing its own first focus.
let wentBack = false;
export const markBack = () => {
  wentBack = true;
};
/** While the screen Back lands on is created: whether it was reached by Back. */
export const cameBack = () => wentBack;
// A page Back lands on gives way to this one (`backThenReplace`).
let afterBack: string | null = null;
/** Back `steps` entries (one by default), and the page there gives way to `to`: one entry where two would stack
 * (the player's Versions after autoplay moved on). */
export function backThenReplace(to: string, steps = 1) {
  afterBack = to;
  markBack();
  history.go(-steps);
}
/** The shell, as Back lands: the page to put in its place, once. */
export function takeAfterBack(): string | null {
  const to = afterBack;
  afterBack = null;
  return to;
}

/** The shell, once the new screen is in: reads and clears the mark. */
export function takeBack(): boolean {
  const was = wentBack;
  wentBack = false;
  return was;
}

// A page holds unsaved edits and asks before it is left: the TV rail's focus alone then opens no tab (app.tsx).
export const [leaveGuarded, setLeaveGuarded] = createSignal(false);

export const historyDepth = () => (history.state as { _depth?: number } | null)?._depth ?? 0;

// Incognito (plans/batch-10-incognito.md 5.5): the router runs from memory. Each entry's address and state live here,
// and the browser's own entry holds only its step at "/": a browser keeps `history.state` and the address in its
// session restore data on disk. Every read of `history.state` and every write through the history calls lands on the
// memory entry of the browser's current step, so Back, Forward and the pages' own entries (an overlay, the palette)
// work as they do outside Incognito. Each run of Incognito has its own number (`_run`): a step left from an earlier run
// is a page from before this one.
type MemoryEntry = { value: string; state: Record<string, unknown> };
let memory: { entries: MemoryEntry[]; listeners: Set<(e: MemoryEntry) => void>; run: number } | null = null;
let runs = 0;
const nativeState = Object.getOwnPropertyDescriptor(History.prototype, "state")?.get;
const browserState = () => nativeState?.call(history) as Record<string, unknown> | null;
const nativePush = history.pushState.bind(history);
/** The browser's entry is one of this run's steps. */
const ours = () => {
  const s = browserState();
  return !!memory && typeof s?._step === "number" && s._run === memory.run && s._step < memory.entries.length;
};
const step = () => (ours() ? (browserState()?._step as number) : 0);
const entryNow = () => memory?.entries[step()];
Object.defineProperty(history, "state", { configurable: true, get: () => entryNow()?.state ?? browserState() });

/** The app's address: the router's in Incognito, the browser's otherwise. */
export const currentPath = () => entryNow()?.value ?? location.pathname + location.search;
const pathOf = (value: string) => value.split(/[?#]/)[0] ?? value;
/** The app's path, without its query: in Incognito the browser's is always "/". */
export const currentPathname = () => pathOf(currentPath());

/** Starts the memory the Incognito router runs from, at Home, as the history its `MemoryRouter` takes. */
export function startMemory() {
  const m = { entries: [{ value: "/", state: { _depth: 0, _profile: shown(), _from: null } as Record<string, unknown> }], listeners: new Set<(e: MemoryEntry) => void>(), run: ++runs };
  memory = m;
  nativePush({ _step: 0, _run: m.run }, "", "/");
  setEntryOwner(stampOf());
  // The router's memory history is typed as strings, but its `createRouter` takes `{value, state}` too (it wraps a
  // string itself), which is how the entries' state reaches `useLocation().state` here.
  return {
    get: () => ({ ...m.entries[step()] }) as unknown as string,
    set: ({ value, replace, state, scroll }: { value: string; replace?: boolean; state?: unknown; scroll?: boolean }) => {
      history[replace ? "replaceState" : "pushState"](state ?? {}, "", value);
      // As the browser's router does: to the hash, else to the top (the phone's page scrolls the document).
      const hash = value.split("#")[1];
      const target = hash ? document.getElementById(decodeURIComponent(hash)) : null;
      if (target) target.scrollIntoView();
      else if (scroll) window.scrollTo(0, 0);
    },
    back: () => history.back(),
    forward: () => history.forward(),
    go: (n: number) => history.go(n),
    listen: (fn: (value: string) => void) => {
      const heard = (e: MemoryEntry) => fn({ ...e } as unknown as string);
      m.listeners.add(heard);
      return () => m.listeners.delete(heard);
    },
  };
}

/** Incognito ended: the browser's own history runs the router again. */
export function endMemory() {
  memory = null;
  setEntryOwner(stampOf());
}

/** A write in Incognito: the memory entry, and only the step in the browser's. */
function writeMemory(push: boolean, state: Record<string, unknown>, url: string | URL | null | undefined) {
  const m = memory as NonNullable<typeof memory>;
  const at = step();
  const value = url == null ? (m.entries[at]?.value ?? "/") : String(url);
  if (!push) {
    m.entries[at] = { value, state: { ...state, _depth: at } };
    return;
  }
  m.entries.length = at + 1;
  m.entries.push({ value, state: { ...state, _depth: at + 1 } });
  nativePush({ _step: at + 1, _run: m.run }, "", "/");
}

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
// A fresh entry's depth as the router stamps it (history.length - 1): a production build may run this module before
// the router's, while the entry has no stamp yet, and a base of 0 let the app's Back leave the app (QA r2, Q7 row 20).
let base = stored === null || !sameVisit ? ((history.state as { _depth?: number } | null)?._depth ?? history.length - 1) : Number(stored);
export const historyBase = () => (memory ? 0 : base);
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
    const from = name === "pushState" ? pathOf(currentPath()) : (fields._from ?? entryFrom());
    if (memory) writeMemory(name === "pushState", { ...fields, _profile: owner, _from: from }, url);
    else native({ ...fields, _profile: owner, _from: from }, unused, url);
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
  screenChanged();
  if (memory) {
    // A page from before Incognito (the browser's Back past its first entry): Incognito starts again at its first one.
    if (!ours()) {
      memory.entries.length = 1;
      nativePush({ _step: 0, _run: memory.run }, "", "/");
    }
    setEntryOwner(stampOf());
    const now = entryNow();
    if (now) for (const fn of memory.listeners) fn(now);
    return;
  }
  const owner = stampOf();
  const active = shown();
  // An entry below the base is from before the app's root (first run's sheets, an old profile's pages): Back never
  // lands on it, Home does (plans/first-screens.md 3.6). A stamped depth only: an entry the router has not stamped is not one.
  const stamped = (history.state as { _depth?: unknown } | null)?._depth;
  const below = typeof stamped === "number" && stamped < base;
  if (below || (open() && owner && active && owner !== active)) {
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

/** A page's own state from its history entry (its version, where it came from), without the stamps
 * the history calls write (`_depth`, `_profile`, `_from`): for a new entry on the same page. */
export const ownState = (state: unknown) => Object.fromEntries(Object.entries((state ?? {}) as object).filter(([k]) => !k.startsWith("_")));
