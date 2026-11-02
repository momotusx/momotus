/** A double tap on a phone: the first tap changes the screen (a page, a step, a sheet that closes) and the second
 * lands on whatever the new screen has under the finger (QA r1: a skipped question, an addon moved). A touch that
 * starts within GHOST_MS of a change a tap made, where that tap was, presses nothing. A tap elsewhere, a mouse and a
 * key are never held back. */

const GHOST_MS = 300;
/** How far from the first tap the second one of a double tap lands: a fingertip. */
const NEAR_PX = 48;
type Touch = { x: number; y: number; t: number };
let lastTouch: Touch | null = null;
// The tap that changed the screen, and when the change came.
let changedBy: Touch | null = null;
let changedAt = -Infinity;
let ghost = false;

/** The screen under the finger just changed. */
export const screenChanged = () => {
  changedAt = performance.now();
  // Only a change that tap itself made: one by Back or by the page a while later is no double tap.
  changedBy = lastTouch && changedAt - lastTouch.t < 500 ? lastTouch : null;
};

window.addEventListener(
  "pointerdown",
  (e) => {
    // A mouse or pen press is the latest input: what it changes was no tap's.
    if (e.pointerType !== "touch") {
      ghost = false;
      lastTouch = null;
      return;
    }
    const now = performance.now();
    ghost = !!changedBy && now - changedAt < GHOST_MS && Math.hypot(e.clientX - changedBy.x, e.clientY - changedBy.y) < NEAR_PX;
    lastTouch = { x: e.clientX, y: e.clientY, t: now };
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
