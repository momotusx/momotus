import { expect, test } from "bun:test";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { byLanguage, offSummary, trackDetail } from "./subtitles";

const track = (language: string, languageName: string, name: string, more: Partial<SubtitleTrack> = {}): SubtitleTrack => ({
  url: `http://127.0.0.1/sub/${language}/${name}`,
  lang: language,
  language,
  languageName,
  nativeName: languageName,
  name,
  label: null,
  source: "OpenSubtitles v3",
  forced: false,
  hearingImpaired: false,
  fits: false,
  ...more,
});

test("each run of one language is a section, indexes kept", () => {
  const tracks = [track("tr", "Turkish", "TOPAZ"), track("en", "English", "OpenSubtitles v3 1"), track("en", "English", "OpenSubtitles v3 2"), track("pt-BR", "Portuguese (Brazil)", "OpenSubtitles v3")];
  const sections = byLanguage(tracks);
  expect(sections.map((s) => [s.name, s.tracks.map((t) => t.index)])).toEqual([
    ["Turkish", [0]],
    ["English", [1, 2]],
    ["Portuguese (Brazil)", [3]],
  ]);
});

test("the detail line names the source only when the name does not", () => {
  expect(trackDetail(track("tr", "Turkish", "TOPAZ", { hearingImpaired: true }))).toBe("OpenSubtitles v3, For the hard of hearing");
  expect(trackDetail(track("en", "English", "OpenSubtitles v3 2"))).toBe("");
  expect(trackDetail(track("en", "English", "OpenSubtitles v3", { forced: true }))).toBe("Forced");
});

test("the Subtitles button counts what is in while the core still asks", () => {
  expect(offSummary(87, true)).toEqual({ text: "Off, 87 found", searching: true });
  expect(offSummary(87, false)).toEqual({ text: "Off, 87 found", searching: false });
  expect(offSummary(0, true)).toEqual({ text: "Searching", searching: false });
  expect(offSummary(0, false)).toEqual({ text: "None found", searching: false });
});
