/** The core's subtitle tracks as the menu shows them: one section per
 * language, in the core's order (preferred languages first, then the rest
 * by name; plans/m13-subtitle-languages.md), and the picker built on them
 * (plans/player-polish.md 2.4). */

import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { languageTag } from "./words";

export type LanguageSection = { language: string; name: string; tracks: { track: SubtitleTrack; index: number }[] };

/** The core keeps a language's tracks together: each run is a section. */
export function byLanguage(tracks: SubtitleTrack[]): LanguageSection[] {
  const sections: LanguageSection[] = [];
  tracks.forEach((track, index) => {
    const last = sections[sections.length - 1];
    if (last?.language === track.language) last.tracks.push({ track, index });
    else sections.push({ language: track.language, name: track.languageName, tracks: [{ track, index }] });
  });
  return sections;
}

/** A track inside the file, as the engine lists it (player/engine.ts). */
type FileTrack = { id: string; lang: string | null; forced?: boolean };

/** One language of the picker's first level: its tag, its English name, its
 * own name when the core knows a different one, the codes a search matches,
 * and its options: the file's own tracks first (once the file has opened),
 * then the core's in the core's order, the best match first. */
export type PickerLanguage<F extends FileTrack = FileTrack> = {
  language: string;
  name: string;
  own: string | null;
  codes: string[];
  file: F[];
  tracks: SubtitleTrack[];
};

/** The picker's languages: the core's, in its order, each with the file's
 * tracks of that language (its code as a tag, "chi" as "zh", joins the
 * core's language of that tag, else the first of that base language,
 * "zh-Hans"); a language only the file has comes after them, named by
 * `nameOf`. */
export function pickerLanguages<F extends FileTrack>(tracks: SubtitleTrack[], file: F[], nameOf: (lang: string) => string): PickerLanguage<F>[] {
  const out: PickerLanguage<F>[] = byLanguage(tracks).map((s) => {
    const first = s.tracks[0]?.track;
    const own = first?.nativeName && first.nativeName !== s.name ? first.nativeName : null;
    const codes = [...new Set([s.language.toLowerCase(), ...s.tracks.map((t) => t.track.lang.toLowerCase())])];
    return { language: s.language, name: s.name, own, codes, file: [], tracks: s.tracks.map((t) => t.track) };
  });
  const base = (tag: string) => tag.toLowerCase().split("-")[0];
  for (const f of file) {
    if (!f.lang) continue;
    const tag = languageTag(f.lang);
    const lang = out.find((l) => l.language.toLowerCase() === tag.toLowerCase()) ?? out.find((l) => base(l.language) === base(tag));
    if (lang) lang.file.push(f);
    else out.push({ language: tag, name: nameOf(f.lang), own: null, codes: [...new Set([tag.toLowerCase(), f.lang.toLowerCase()])], file: [f], tracks: [] });
  }
  return out;
}

/** Lower case with the accents gone, so "turkce" finds "Türkçe". */
const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
/** Whether `query` starts a word of `text`. */
const startsWord = (text: string, query: string) => ` ${fold(text).replace(/[^\p{L}\p{N}]+/gu, " ")}`.includes(` ${query}`);

/** The search field's rule: a word of the English name or of the language's
 * own name starts with what was typed, or a code does as typed ("pt",
 * "pob", "deu", "pt-br", "es-419"). */
export function matchesLanguage(l: Pick<PickerLanguage, "name" | "own" | "codes">, typed: string): boolean {
  const code = fold(typed.trim());
  const q = code.replace(/[^\p{L}\p{N}]+/gu, " ").trim();
  if (!q) return true;
  return startsWord(l.name, q) || (!!l.own && startsWord(l.own, q)) || l.codes.some((c) => c.startsWith(code) || c.startsWith(q));
}

/** What a language's row counts: every option it opens. */
export const optionCount = (l: PickerLanguage) => l.file.length + l.tracks.length;

/** What picking a language turns on: the file's own track that is not
 * forced, once the file has opened (it always fits the file), else the
 * core's best match, else a forced one of the file's. */
export function bestOption<F extends FileTrack>(l: PickerLanguage<F>): { file: F } | { track: SubtitleTrack } | null {
  const file = l.file.find((f) => !f.forced);
  if (file) return { file };
  const track = l.tracks[0];
  if (track) return { track };
  return l.file[0] ? { file: l.file[0] } : null;
}

/** What the start turns on for the core's `autoSubtitle` (plans/player-polish.md
 * 2.2): the file's own track of that language when there is one that is not
 * forced (it wins once the file has opened), else the core's track itself. */
export function startOption<F extends FileTrack>(languages: PickerLanguage<F>[], auto: string): { file: F } | { track: SubtitleTrack } | null {
  const lang = languages.find((l) => l.tracks.some((t) => t.url === auto));
  const track = lang?.tracks.find((t) => t.url === auto);
  if (!lang || !track) return null;
  const file = lang.file.find((f) => !f.forced);
  return file ? { file } : { track };
}

/** The Subtitles button with no track on: the languages the picker shows as
 * soon as one is in, `searching` while the core still asks
 * (`subtitlesPending`), and "Searching" only while none is in. */
export function offSummary(languages: number, pending: boolean): { text: string; searching: boolean } {
  if (languages) return { text: `Off, ${languages} ${languages === 1 ? "language" : "languages"}`, searching: pending };
  return { text: pending ? "Searching" : "None found", searching: false };
}

/** `subtitleChosen` for a track inside the file: no core URL, and the file's own code for its
 * language (`eng`, `pt-BR`) when it gives one, so the profile's languages follow the pick. */
export const fileChoice = (lang: string | null) => ({ type: "subtitleChosen", url: null, ...(lang ? { language: lang } : {}) });

/** The core's words for a track that names the playing release's group. */
export const FITS = "Same release group";
export const MATCHES = "Matches this file";

/** `text` cut to `max` characters in the middle, at a dot or a space, so a
 * release's start and its group at the end both stay in view. */
export function middle(text: string, max: number): string {
  if (text.length <= max) return text;
  const at = (j: number) => text[j] === "." || text[j] === " ";
  let from = text.length - Math.max(1, max - 1 - Math.floor((max - 1) * 0.45));
  while (from > 0 && !at(from - 1)) from--;
  // A tail with no boundary near enough keeps its share and starts at the next boundary instead.
  if (text.length - from > max - 5) for (from = text.length - (max - 5); from < text.length && !at(from - 1); from++);
  const tail = text.slice(from).replace(/^[. ]+/, "");
  let cut = max - 1 - tail.length;
  while (cut > 0 && !at(cut)) cut--;
  const head = text.slice(0, cut > 0 ? cut : max - 1 - tail.length).replace(/[. ]+$/, "");
  return `${head}…${tail}`;
}

/** The quiet line under a track's name, in parts: only what tells tracks apart (what kind of file it
 * is, the core's match words, then the release text cut in the middle to the line's `max` characters,
 * the whole line when the rest leaves it under 24). No addon or service is named where a viewer reads
 * it, so the source never shows. */
export function trackDetail(t: SubtitleTrack, max = 46): string[] {
  // A rate as written ("23.976", "25"), not the core's float (23.976000785827637).
  const made = t.madeForFps ? `Made for ${Number(t.madeForFps.toFixed(3))} fps` : null;
  const parts = [t.forced ? "Forced" : null, t.hearingImpaired ? "For the hard of hearing" : null, t.hashMatch ? MATCHES : null, t.fits ? FITS : null, made].filter(
    (p): p is string => !!p,
  );
  if (!t.release || t.release === t.name) return parts;
  const room = max - parts.join(" · ").length - 3;
  return [...parts, middle(t.release, room >= 24 ? room : max)];
}

/** A language's flag file in circle-flags' language set (`has` says which
 * exist): the tag itself ("pt-br"), else its language ("zh" for "zh-Hant"). */
export function flagFile(tag: string, has: (name: string) => boolean): string | null {
  const lower = tag.toLowerCase();
  const base = lower.split("-")[0] ?? lower;
  return has(lower) ? lower : has(base) ? base : null;
}
