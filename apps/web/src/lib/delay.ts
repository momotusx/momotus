/** The subtitle timing stepper, as Stremio's (stremio-web subtitleDelay.ts,
 * plans/research/subtitle-sync.md 6.1): 0.1 s a press; held, the first repeat
 * comes after 250 ms and then one every 100 ms, each moving 0.1 s times
 * min(whole seconds held + 3, 6); within 60 s either way. Plus means the
 * subtitles come later. */

export const DELAY_STEP = 0.1;
export const MAX_DELAY = 60;
export const FIRST_REPEAT_MS = 250;
export const REPEAT_MS = 100;

/** What one repeat moves after `heldMs` of holding: 0.3 s a tick in the first
 * second, 0.4 and 0.5 in the next two, 0.6 from the third on. */
export const holdStep = (heldMs: number) => DELAY_STEP * Math.min(Math.floor(heldMs / 1000) + 3, 6);

/** `value` moved by `by`, snapped to 0.1 s and kept within 60 s either way. */
export function nudge(value: number, by: number): number {
  const next = Math.round((value + by) * 10) / 10;
  return Math.min(MAX_DELAY, Math.max(-MAX_DELAY, next)) || 0;
}

/** The value as the stepper shows it: "+0.3 s", "−1.2 s" (a true minus sign), "0.0 s". */
export const delayText = (sec: number) => `${sec > 0 ? "+" : sec < 0 ? "−" : ""}${Math.abs(sec).toFixed(1)} s`;

/** The value in words, for a screen reader. */
export const delayWords = (sec: number) => (sec === 0 ? "Subtitles in step" : `Subtitles ${Math.abs(sec).toFixed(1)} seconds ${sec > 0 ? "later" : "earlier"}`);
