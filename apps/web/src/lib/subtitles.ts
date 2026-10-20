/** The core's subtitle tracks as the menu shows them: one section per
 * language, in the core's order (preferred languages first, then the rest
 * by name; plans/m13-subtitle-languages.md). */

import type { SubtitleTrack } from "../core/generated/SubtitleTrack";

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

/** The Subtitles button with no track on: the count as soon as one track is
 * in, `searching` while the core still asks (`subtitlesPending`), and
 * "Searching" only while none is in. */
export function offSummary(found: number, pending: boolean): { text: string; searching: boolean } {
  if (found) return { text: `Off, ${found} found`, searching: pending };
  return { text: pending ? "Searching" : "None found", searching: false };
}

/** The line under a track's name: its source when the name does not
 * already say it, and what kind of file it is. */
export function trackDetail(t: SubtitleTrack): string {
  const source = t.name === t.source || t.name.startsWith(`${t.source} `) ? null : t.source;
  return [source, t.forced ? "Forced" : null, t.hearingImpaired ? "For the hard of hearing" : null].filter(Boolean).join(", ");
}
