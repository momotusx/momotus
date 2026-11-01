/** A double tap on a phone: the first tap changes the screen (a page, a step, a sheet that closes) and the second
 * lands on whatever the new screen has under the finger (QA r1: a skipped question, an addon moved). A touch that
 * starts within GHOST_MS of a change presses nothing. A mouse or a key is never held back. */

const GHOST_MS = 300;
let changedAt = -Infinity;
let ghost = false;

/** The screen under the finger just changed. */
export const screenChanged = () => {
  changedAt = performance.now();
};

window.addEventListener(
  "pointerdown",
  (e) => {
    ghost = e.pointerType === "touch" && performance.now() - changedAt < GHOST_MS;
  },
  { capture: true },
);
// A touch that became a scroll clicks nothing: the next click is not its.
window.addEventListener("pointercancel", () => void (ghost = false), { capture: true });
window.addEventListener(
  "click",
  (e) => {
    if (!ghost) return;
    ghost = false;
    e.preventDefault();
    e.stopImmediatePropagation();
  },
  { capture: true },
);
