/** Small, pure formatters shared by screens. */

import type { Ratings } from "../core/generated/Ratings";

/** A stream's size in the 1024-based units addons print: 5368709120 is
 * "5 GB", as Torrentio lists it and as the size limit reads. */
export function bytes(n: number): string {
  const gb = n / 1024 ** 3;
  if (gb >= 100) return `${Math.round(gb)} GB`;
  if (gb >= 1) return `${Number(gb.toFixed(1))} GB`;
  if (n >= 1024 ** 2) return `${Math.round(n / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(n / 1024))} KB`;
}

/** A title's scores side by side as plain text, the source first: "IMDb 8.8",
 * "RT 87%", with what a screen reader says ("Rotten Tomatoes 87 percent"), the
 * source and the score apart for the ratings row. The core sends Rotten Tomatoes as
 * a number or not at all, never a word. A
 * score the core left null has no slot, and the brands' logos and icons never appear
 * (plans/research/ratings.md). */
export type RatingPart = { text: string; source: string; score: string; spoken: string; imdb: boolean };
export function ratingParts(r: Ratings | null | undefined): RatingPart[] {
  const parts: RatingPart[] = [];
  if (r?.imdb) parts.push({ text: `IMDb ${r.imdb}`, source: "IMDb", score: r.imdb, spoken: `IMDb ${r.imdb}`, imdb: true });
  const rt = r?.rottenTomatoes;
  if (rt) parts.push({ text: `RT ${rt}`, source: "RT", score: rt, spoken: `Rotten Tomatoes ${rt.replace("%", " percent")}`, imdb: false });
  return parts;
}

/** What a screen reader says for a poster: its name and caption, then the
 * scores its cover shows ("Inception, 2010, IMDb 8.8, Rotten Tomatoes 87 percent"). */
export function posterLabel(parts: (string | null | undefined)[], r: Ratings | null | undefined): string {
  return [...parts.filter((p): p is string => !!p), ...ratingParts(r).map((p) => p.spoken)].join(", ");
}

/** 161 -> "2:41", 3725 -> "1:02:05". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** An episode: its season when the id names one (`kitsu:44042:5` names none). */
export interface VideoRef {
  season: number | null;
  episode: number;
}

/** Season and episode from what follows the title's own id in a video id,
 * the core's rule (mq-runtime versions.rs `episode`): `tt0944947:2:3` is S2
 * E3, `kitsu:44042:5` (the title `kitsu:44042`) is episode 5 with no season.
 * Anything else is a movie (`null`). */
export function parseVideo(id: string, videoId: string): VideoRef | null {
  if (!videoId.startsWith(id)) return null;
  const numbers = videoId
    .slice(id.length)
    .split(":")
    .filter((p) => /^\d+$/.test(p))
    .map(Number);
  if (numbers.length === 2) return { season: numbers[0] as number, episode: numbers[1] as number };
  if (numbers.length === 1) return { season: null, episode: numbers[0] as number };
  return null;
}

export const episodeLabel = (season: number | null, episode: number) => (season === null ? `E${episode}` : `S${season}:E${episode}`);

// Ids keep their colons ("tt0944947:1:1"): they are valid in a path segment.
const seg = (s: string) => encodeURIComponent(s).replace(/%3A/gi, ":");

export const titlePath = (kind: string, id: string) => `/title/${seg(kind)}/${seg(id)}`;

export const playPath = (kind: string, id: string, videoId: string) =>
  `/play/${seg(kind)}/${seg(id)}/${seg(videoId)}`;

/** A version picked by hand rides in the history entry's state, never in
 * the address: a stream key can carry an addon's account key, and the
 * address bar and history are every profile's and sync with the browser.
 * With no state (a shared link) the player plays the recommendation. */
export const playState = (key: string) => ({ key });

export const versionsPath = (kind: string, id: string, videoId: string) =>
  `${titlePath(kind, id)}/versions/${seg(videoId)}`;

/** Route params arrive as written in the URL; ids may hold escapes. */
export function decoded(s: string | undefined): string {
  try {
    return decodeURIComponent(s ?? "");
  } catch {
    return s ?? "";
  }
}

export const pct = (x: number) => `${Math.round(Math.min(1, Math.max(0, x)) * 1000) / 10}%`;

/** Cinemeta runtimes look like "57 min" or "1h 52min". */
export function runtimeMinutes(runtime: string | null | undefined): number | null {
  if (!runtime) return null;
  const h = /(\d+)\s*h/.exec(runtime);
  const m = /(\d+)\s*m/.exec(runtime);
  const total = (h ? Number(h[1]) * 60 : 0) + (m ? Number(m[1]) : 0);
  return total || null;
}

/** Release names break between tokens so a focused row never truncates:
 * after a dot unless a digit follows ("5.1", "H.265", "S02E03.2160p" stay whole), and before
 * the release group ("-FLUX"). */
export function releaseParts(name: string): string[] {
  return name
    .replace(/\.(?!\d)/g, ".\u0000")
    .replace(/-(?=[^-.]+$)/, "\u0000-")
    .split("\u0000");
}

export function seasonName(season: number): string {
  return season === 0 ? "Specials" : `Season ${season}`;
}

/** Sources that failed the same way, from one addon, once each in first order, with their count. */
export function groupFailures(attempts: readonly { via: string; error: string }[]): { via: string; words: string }[] {
  const seen = new Map<string, { via: string; error: string; count: number }>();
  for (const a of attempts) {
    const id = `${a.via}\u0000${a.error}`;
    const found = seen.get(id);
    if (found) found.count += 1;
    else seen.set(id, { via: a.via, error: a.error, count: 1 });
  }
  return [...seen.values()].map((f) => ({ via: f.via, words: f.count > 1 ? `${f.error} · ${f.count} sources` : f.error }));
}

/** An air date ("YYYY-MM-DD", read in UTC as the core writes it): "Oct 27, 2026". */
export const airDate = (released: string) =>
  new Date(released).toLocaleDateString("en", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });

/** When the next episode starts on its own: at the end of this one, then
 * counted down over the last 15 s ("Starts in 12 s"). */
export const startsText = (leftSec: number) => (leftSec > 15 ? "Starts when this ends" : `Starts in ${Math.max(0, Math.ceil(leftSec))} s`);
