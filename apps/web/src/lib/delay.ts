/** Subtitle timing, in two steps on one 0.05 s grid (plans/player-bar.md D2, D3).
 *
 * The full picker's fine stepper, as Stremio's (stremio-web subtitleDelay.ts,
 * plans/research/subtitle-sync.md 6.1): 0.1 s a press; held, the first repeat
 * comes after 350 ms and then one every 100 ms, each moving 0.1 s times
 * min(whole seconds held + 3, 6).
 *
 * The bar's quick shift (round3.html `hold`): 0.25 s a press; held, the first
 * repeat comes after 400 ms and then one every 150 ms, each moving 0.25 s times
 * min(whole seconds held + 1, 4), so a 2.0 s hold moves exactly 4.5 s.
 *
 * Within 60 s either way. Plus means the subtitles come later. */

export const MAX_DELAY = 60;
/** How long a shift rests before its value goes to the core (plans/core-protocol.md `subtitleDelay`). */
export const DELAY_SETTLE_MS = 700;

/** A press's step and how a hold repeats it: `first` ms to the first repeat, then one every `every` ms, each
 * `step` times `ramp(ms held)`. */
export type HoldRule = { step: number; first: number; every: number; ramp: (heldMs: number) => number };
export const FINE: HoldRule = { step: 0.1, first: 350, every: 100, ramp: (ms) => Math.min(Math.floor(ms / 1000) + 3, 6) };
export const QUICK: HoldRule = { step: 0.25, first: 400, every: 150, ramp: (ms) => Math.min(Math.floor(ms / 1000) + 1, 4) };
export const DELAY_STEP = FINE.step;
export const QUICK_STEP = QUICK.step;

/** What one repeat of the fine stepper moves after `heldMs` of holding: 0.3 s a tick in the first
 * second, 0.4 and 0.5 in the next two, 0.6 from the third on. */
export const holdStep = (heldMs: number) => FINE.step * FINE.ramp(heldMs);

/** The timers a hold runs on (the page's own, or a test's). */
export type Clock = { after: (ms: number, f: () => void) => number; cancel: (id: number) => void; now: () => number };
const pageClock: Clock = { after: (ms, f) => window.setTimeout(f, ms), cancel: (id) => window.clearTimeout(id), now: () => performance.now() };

/** A press or hold by `rule`: `start` on the press, `stop(true)` on its release (one step when no
 * repeat came yet), `stop(false)` when it is cut off. `move` gets each step with its sign. A key's own
 * repeats never reach it: the caller starts once per press, so the speed never depends on the keyboard
 * or the remote. */
export function hold(move: (by: number) => void, clock: Clock = pageClock, rule: HoldRule = FINE) {
  let timer = 0;
  let dir = 0;
  let since = 0;
  let repeated = false;
  const stop = (single: boolean) => {
    clock.cancel(timer);
    if (dir && single && !repeated) move(dir * rule.step);
    dir = 0;
  };
  const start = (d: number) => {
    stop(false);
    dir = d;
    since = clock.now();
    repeated = false;
    const tick = () => {
      repeated = true;
      move(d * rule.step * rule.ramp(clock.now() - since));
      timer = clock.after(rule.every, tick);
    };
    timer = clock.after(rule.first, tick);
  };
  return { start, stop, held: () => dir !== 0 };
}

/** `value` moved by `by`, snapped to 0.05 s (both steps land on it) and kept within 60 s either way. */
export function nudge(value: number, by: number): number {
  const next = Math.round((value + by) * 20) / 20;
  return Math.min(MAX_DELAY, Math.max(-MAX_DELAY, next)) || 0;
}

/** The size with at least one decimal and at most two: "0.25", "0.5", "1.0". */
const amount = (sec: number) => Math.abs(sec).toFixed(2).replace(/(\.\d)0$/, "$1");

/** The value as it shows: "+0.25 s", "−1.5 s" (a true minus sign), "0.0 s". */
export const delayText = (sec: number) => `${sec > 0 ? "+" : sec < 0 ? "−" : ""}${amount(sec)} s`;

/** The value in words, for a screen reader, signed as Apple says it: "Plus 0.3 seconds",
 * "Minus 1.25 seconds", "0.0 seconds" (the full picker's stepper). */
export const delayWords = (sec: number) => `${sec > 0 ? "Plus " : sec < 0 ? "Minus " : ""}${amount(sec)} seconds`;

/** The size alone, unsigned, for the bar's spoken lines ("Subtitles 0.25 seconds later"). */
export const delayAmount = amount;
