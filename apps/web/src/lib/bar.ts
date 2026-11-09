/** The player bar's rules (plans/player-bar.md), kept apart from the screen so each can be tested: when the quick
 * shift and Next show (D4, D14), which languages the subtitle strip offers (D5), how the row fits a narrow window or
 * phone (D16, D17), the Position steps (D7) and the shift's spoken words (D12, section 8). */

import type { NextEpisode } from "../core/generated/NextEpisode";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import type { Form } from "../form";
import { delayAmount, delayText } from "./delay";
import { type PickerLanguage, sameLanguage } from "./subtitles";
import { choiceWords } from "./words";

export const barWords = choiceWords.player.bar;
const names = barWords.names;
const fill = (line: string, fields: Record<string, string>) => line.replace(/\{(\w+)\}/g, (all, k: string) => fields[k] ?? all);

/** D4: the shift shows exactly where the full picker's Timing row does, a core track that is on and listed. The
 * file's own tracks (mpv) cannot be delayed here, and with subtitles off there is nothing to time. */
export const shiftShown = (on: string | null | undefined, tracks: SubtitleTrack[]): boolean => !!on && tracks.some((t) => t.url === on);

/** D14: Next shows only when the core names the next episode (the same `next` the Up Next card plays). */
export const nextShown = (next: NextEpisode | null | undefined): next is NextEpisode => !!next;

/** D5: how many languages the strip lists before More languages. */
export const chipCap = (form: Form) => (form === "phone" ? 3 : 5);

/** D5: the strip's languages after Off: the one on now, then the profile's subtitle languages that have tracks (in
 * the profile's order), then the core's order; at most `cap`. They come from the picker's own list, so offline and
 * while the list grows they show what it shows. */
export function stripLanguages<L extends Pick<PickerLanguage, "language">>(languages: L[], on: string | null, profile: string[], cap: number): L[] {
  const out: L[] = [];
  const add = (l: L | undefined) => l && !out.includes(l) && out.push(l);
  add(languages.find((l) => l.language === on));
  for (const p of profile) add(languages.find((l) => sameLanguage(l.language, p)));
  for (const l of languages) add(l);
  return out.slice(0, cap);
}

/** How the bar's row fits (D16): each side's pills keep their words while the side fits its column, the shift's
 * words turn to chevrons when the right side still overflows, and a side that still overflows puts the right
 * group on a row of its own. `left` and `right` are the groups' widths in each state (the shift's room and the
 * longest values always counted), `column` what each side has: (width minus the transport) / 2. */
export type Fit = { leftWords: boolean; rightWords: boolean; chevrons: boolean; rows: 1 | 2 };
export function desktopFit(column: number, left: { words: number; icons: number }, right: { words: number; icons: number; chevrons: number }): Fit {
  const leftWords = left.words <= column;
  const rightWords = right.words <= column;
  const chevrons = !rightWords && right.icons > column;
  const l = leftWords ? left.words : left.icons;
  const r = rightWords ? right.words : chevrons ? right.chevrons : right.icons;
  return { leftWords, rightWords, chevrons, rows: l > column || r > column ? 2 : 1 };
}

/** The scrubber's least width on a phone's one row (D17). */
export const MIN_SCRUBBER = 120;

/** A phone sideways (D17): one row (the groups, the times and the scrubber) while the scrubber keeps 120 px; first
 * the shift's words turn to chevrons, then the bar takes two rows (the scrubber, then the controls), where the
 * words come back when the row holds them. `width` is the row's, `times` what the clocks and the gaps take of it on
 * one row. */
export function phoneFit(width: number, times: number, left: number, right: { words: number; chevrons: number }): Fit {
  const fits = (r: number) => width - times - left - r >= MIN_SCRUBBER;
  if (fits(right.words)) return { leftWords: false, rightWords: false, chevrons: false, rows: 1 };
  if (fits(right.chevrons)) return { leftWords: false, rightWords: false, chevrons: true, rows: 1 };
  return { leftWords: false, rightWords: false, chevrons: left + right.words > width, rows: 2 };
}

/** D7: the Position steps, percent of the picture over its foot; 8 is today's rule and reads "Default". */
export const POSITIONS = [5, 8, 12, 16, 20, 25];
export const positionText = (p: number) => (p === 8 ? barWords.default : fill(barWords.percent, { n: String(p) }));
/** The step below or above `p` (a value off the steps goes to the nearest one that way), or null at an end. */
export function positionStep(p: number, dir: -1 | 1): number | null {
  const next = dir > 0 ? POSITIONS.find((x) => x > p) : [...POSITIONS].reverse().find((x) => x < p);
  return next ?? null;
}

/** D12: the one polite line said once a shift settles: "Subtitles 0.25 seconds later", or in sync at 0. */
export const shiftSaid = (sec: number) =>
  sec === 0 ? barWords.saidInSync : fill(sec > 0 ? barWords.saidLater : barWords.saidEarlier, { n: delayAmount(sec) });

/** Section 8: the value's name starts with its text: "+0.25 s, subtitles later, press to reset"; on TV "press OK to
 * adjust" (Reset is its own chip there). */
export function shiftName(sec: number, tv: boolean): string {
  const value = delayText(sec);
  const base = fill(sec > 0 ? names.valueLater : sec < 0 ? names.valueEarlier : names.valueInSync, { value });
  if (tv) return `${base}, ${names.pressOkToAdjust}`;
  return sec === 0 ? base : `${base}, ${names.pressToReset}`;
}
