/** One Back for every overlay (plans/design-v3-phase34-web.md, the panel's contract): an overlay
 * pushes its close while it is open, and the remote's or the keyboard's Back closes the newest one,
 * whatever order they mounted in. One capture listener stops the key, so the app's own Back (a
 * page's history) never runs for it.
 *
 * An overlay that takes `entry` is also a history entry on the page's address, as the Search
 * palette is (screens/search.tsx): the browser's Back (a phone's edge swipe, a mouse's Back button)
 * closes it and the page stays. */

import { isBackKey } from "./dom";

type Entry = { close: () => void; mark?: number };
type State = { _overlay?: number; _depth?: number } | null;

const stack: Entry[] = [];
let listening = false;
let marks = 0;
// Entries this module dropped itself (an overlay closed by its key or its button): their popstate is ours.
let dropping = 0;
// A Back key closed an overlay and is still down: its repeats are swallowed, so a held Back closes one
// thing and never goes on to leave the page.
let held = false;
const marked = () => (history.state as State)?._overlay;

// A reload on an overlay's entry: the overlay is gone, so is its mark.
if (typeof history !== "undefined" && marked()) history.replaceState({ ...(history.state as object), _overlay: undefined }, "");

function listen() {
  listening = true;
  window.addEventListener(
    "keydown",
    (e) => {
      if (!isBackKey(e)) return;
      const top = stack[stack.length - 1];
      if (!top && !(held && e.repeat)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.repeat || !top) return;
      held = true;
      top.close();
    },
    { capture: true },
  );
  window.addEventListener("keyup", (e) => {
    if (isBackKey(e)) held = false;
  });
}

// Registered at load, before the router's and the shell's listeners (the palette imports this module
// first): leaving an overlay's entry for its page is not a screen's Back.
if (typeof window !== "undefined")
  window.addEventListener("popstate", (e) => {
    if (dropping > 0) {
      dropping--;
      e.stopImmediatePropagation();
      return;
    }
    const top = stack[stack.length - 1];
    if (!top?.mark || marked() === top.mark) return;
    e.stopImmediatePropagation();
    // Its entry is already gone: closing must not go back again.
    top.mark = undefined;
    top.close();
  });

/** `close` runs on the next Back while it is the newest entry; the returned function removes it (and
 * drops its history entry when it is still the current one). */
export function pushBack(close: () => void, opts: { entry?: boolean } = {}): () => void {
  if (!listening) listen();
  const entry: Entry = { close };
  if (opts.entry) {
    entry.mark = ++marks;
    const state = history.state as State;
    history.pushState({ ...state, _overlay: entry.mark, _depth: (state?._depth ?? 0) + 1 }, "");
  }
  stack.push(entry);
  return () => {
    const i = stack.lastIndexOf(entry);
    if (i >= 0) stack.splice(i, 1);
    if (entry.mark && marked() === entry.mark) {
      dropping++;
      history.back();
    }
  };
}
