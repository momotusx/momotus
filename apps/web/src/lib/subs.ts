/** SubRip to WebVTT, so addon subtitles play in a <track>. VTT input passes
 * through unchanged. */
export function toVtt(text: string): string {
  const body = text.replace(/^﻿/, "").replace(/\r\n?/g, "\n").trim();
  if (body.startsWith("WEBVTT")) return body;
  // SRT times use a comma before milliseconds; VTT uses a dot.
  const cues = body.replace(/(\d{1,2}:\d{2}:\d{2}),(\d{3})/g, "$1.$2");
  return `WEBVTT\n\n${cues}\n`;
}
