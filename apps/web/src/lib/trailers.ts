/** Home's trailer previews (plans/appletv-polish.md decisions 5, 13, 15): when one may play, where it
 * plays, and the device's own switch. */
import type { Form } from "../form";

/** Where the hero's trailer plays, per form (the owner's pick, 2026-10-24, plans/appletv-polish.md 4.2):
 * "cover" fills the hero's own frame where the key art sits, cropped to fill, with no YouTube look. */
export type TrailerLayout = "cover" | "none";

// The web TV form stays off until a real TV is measured (decision 5).
export const TRAILER_LAYOUT: Record<Form, TrailerLayout> = { phone: "cover", desktop: "cover", tv: "none" };

/** YouTube's start controls are gone from the clipped frame by 4 s after it plays (Phase 0): the art
 * above the player fades only then. */
export const REVEAL_MS = 4500;
/** The player past the frame on every side, so YouTube's top and bottom bars fall outside the clip. */
export const OVERSCAN = 1.12;

/** The player's size to cover a `w` by `h` frame: 16:9, as tall as the frame (or as wide) times the
 * overscan, centered and clipped by the frame. */
export function coverSize(w: number, h: number): { width: number; height: number } {
  const height = Math.max(h, (w * 9) / 16) * OVERSCAN;
  return { width: (height * 16) / 9, height };
}

/** What the page can tell about this moment; each one false blocks a preview. */
export interface TrailerSignals {
  /** The setting, on this device only. */
  enabled: boolean;
  /** The hero at least half on screen (YouTube's own rule) and the tab shown. */
  visible: boolean;
  reducedMotion: boolean;
  /** `prefers-reduced-data`, Save-Data or a cellular connection where the browser tells. */
  metered: boolean;
  /** A walk or an e2e run: previews stay off unless the run turns them on itself. */
  walk: boolean;
  layout: TrailerLayout;
  /** Enough memory for a second video engine: 6 GB or more where the browser tells (decision 5, 4.2). */
  capable: boolean;
  /** The page has a trailer and has not played it to its end this session or failed. */
  hasTrailer: boolean;
  /** Incognito plays no trailer: YouTube's player keeps its cookies and cache after it (plans/batch-10-incognito.md 5.5). */
  incognito: boolean;
}

export function trailerAllowed(s: TrailerSignals): boolean {
  return !s.incognito && s.enabled && s.capable && s.visible && !s.reducedMotion && !s.metered && !s.walk && s.layout !== "none" && s.hasTrailer;
}

const KEY = "mq.trailerPreviews";

/** The device's own switch, default on (the web TV form has its layout off instead). */
export function previewsOn(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function setPreviewsOn(on: boolean) {
  try {
    localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // Storage blocked: the switch holds for this page only.
  }
}

/** The connection says to spare data: the browser's data saver, or a cellular link where it tells. */
export function meteredNow(): boolean {
  const c = (navigator as Navigator & { connection?: { saveData?: boolean; type?: string } }).connection;
  if (c?.saveData || c?.type === "cellular") return true;
  return typeof matchMedia === "function" && matchMedia("(prefers-reduced-data: reduce)").matches;
}

/** The device holds the player's +197 MB (Pixel, Phase 0): 6 GB or more by `navigator.deviceMemory`,
 * which Chromium reports in steps; a browser that does not tell is taken as able. */
export function capableNow(): boolean {
  const gb = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  return gb === undefined || gb >= 6;
}

/** Where a seek by `by` seconds from `t` lands in a `d` second trailer: never before its start, and never into its
 * last 2 s, where the player closes it (plans/trailer-inline.md 7). */
export function seekTarget(t: number, d: number, by: number): number {
  return Math.min(Math.max(0, t + by), Math.max(0, d - 2));
}

/** One step of a held arrow on a `d` second trailer, after `repeat` repeats: the film player's growing step (10, 20,
 * 30, then 60 s), held to a tenth of the trailer so a held key never leaps to its end (as Android's). */
export function heldStep(repeat: number, d: number): number {
  const grown = repeat < 3 ? 10 : repeat < 6 ? 20 : repeat < 10 ? 30 : 60;
  return Math.min(grown, Math.max(10, d / 10));
}

/** A walk or an e2e run: no trailer plays by itself and none makes a sound, unless the run asked for trailers
 * (`mq.trailerPreviews` "e2e"). */
export function walkNow(): boolean {
  try {
    return navigator.webdriver && localStorage.getItem(KEY) !== "e2e";
  } catch {
    return navigator.webdriver;
  }
}

/** A TV form's size floor for a title header's trailer, in GB as `navigator.deviceMemory` reports it: the 4 GB class
 * (plans/trailer-inline.md 4a; a judgment, the browser's own memory handling is the safety). */
export const TV_HEADER_GB = 4;

/** A TV browser plays a title header's trailer only where it says it has the floor: one that does not tell, as many TV
 * browsers do not, is taken as unable (the Trailer action still plays full screen). */
export function headerOnTvNow(): boolean {
  const gb = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  return gb !== undefined && gb >= TV_HEADER_GB;
}

/** A YouTube video id as YouTube writes it, or none: it goes into a player's URL. */
export function youtubeId(raw: string | null | undefined): string | null {
  return raw && /^[A-Za-z0-9_-]{11}$/.test(raw) ? raw : null;
}

/** A trailer's name on its chip: Main trailer, then Trailer 2, 3 and on. */
export const trailerName = (i: number) => (i === 0 ? "Main trailer" : `Trailer ${i + 1}`);

/** Said when the layer's own pick moved past the first `n` trailers because YouTube refused them here, so the one
 * playing is never a silent swap (QA r3, R3-2); none when it moved past none. */
export function skippedLine(n: number): string | null {
  if (n <= 0) return null;
  const names = Array.from({ length: n }, (_, i) => trailerName(i));
  const list = names.length === 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
  return `${list} can't play here.`;
}
