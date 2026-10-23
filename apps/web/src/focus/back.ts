/** One Back for every overlay (plans/design-v3-phase34-web.md, the panel's contract): an overlay
 * pushes its close while it is open, and the remote's or the keyboard's Back closes the newest one,
 * whatever order they mounted in. One capture listener stops the key, so the app's own Back (a
 * page's history) never runs for it. */

import { isBackKey } from "./dom";

const stack: (() => void)[] = [];
let listening = false;

/** `close` runs on the next Back while it is the newest entry; the returned function removes it. */
export function pushBack(close: () => void): () => void {
  if (!listening) {
    listening = true;
    window.addEventListener(
      "keydown",
      (e) => {
        const top = stack[stack.length - 1];
        if (!top || !isBackKey(e)) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        top();
      },
      { capture: true },
    );
  }
  stack.push(close);
  return () => {
    const i = stack.lastIndexOf(close);
    if (i >= 0) stack.splice(i, 1);
  };
}
