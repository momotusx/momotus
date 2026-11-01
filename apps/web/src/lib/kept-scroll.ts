/** Where a kept page (Home, Discover, Library) was scrolled to, so Back lands there again. The phone scrolls the
 * document, the other forms the page itself. A page's rows can come in a few frames after it opens (Discover's grid),
 * and the browser's own restore on Back puts the document where the next page left it (the top): the scroll is put
 * back on every frame until it has held for half a second, the viewer moves, or 3 s pass. */

import { onCleanup } from "solid-js";
import { form } from "../form";
import { currentPathname } from "../history";

const HOLD_MS = 3000;
const SETTLED_MS = 500;
const INPUTS = ["wheel", "touchstart", "pointerdown", "keydown"] as const;

export function keptScroll() {
  let top = 0;
  /** The page's `ref`. */
  const attach = (el: HTMLElement) => {
    const doc = form() === "phone";
    const scroller: HTMLElement | Window = doc ? window : el;
    const at = () => (doc ? window.scrollY : el.scrollTop);
    // Only while this page is the address: the next page's scroll to its top is not this page's.
    const here = currentPathname();
    const target = top;
    let restoring = target > 0;
    const record = () => {
      if (!restoring && currentPathname() === here) top = at();
    };
    const stop = () => {
      restoring = false;
    };
    scroller.addEventListener("scroll", record, { passive: true });
    for (const type of INPUTS) window.addEventListener(type, stop, { capture: true, passive: true });
    const until = performance.now() + HOLD_MS;
    let held = performance.now();
    const step = () => {
      if (!restoring) return;
      const now = performance.now();
      if (Math.abs(at() - target) >= 2) {
        scroller.scrollTo({ top: target, behavior: "instant" });
        held = now;
      }
      if (now - held > SETTLED_MS || now > until) restoring = false;
      else requestAnimationFrame(step);
    };
    if (restoring) queueMicrotask(step);
    onCleanup(() => {
      restoring = false;
      scroller.removeEventListener("scroll", record);
      for (const type of INPUTS) window.removeEventListener(type, stop, { capture: true });
    });
  };
  return { attach, reset: () => void (top = 0) };
}
