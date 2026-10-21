import { expect, test } from "bun:test";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { bestOption, byLanguage, FITS, flagFile, matchesLanguage, middle, offSummary, optionCount, pickerLanguages, trackDetail } from "./subtitles";

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
  hashMatch: false,
  madeForFps: null,
  delayMs: 0,
  release: null,
  ...more,
});

// What a picker shows for a typical answer: Turkish (the profile's), English, then the rest by name.
const tracks = [
  track("tr", "Turkish", "TOPAZ", { lang: "tur", nativeName: "Türkçe" }),
  track("en", "English", "CAKES, WEB 1080p", { lang: "eng", fits: true }),
  track("en", "English", "PSA, WEBRip 1080p 10 bit", { lang: "eng" }),
  track("pt-BR", "Portuguese (Brazil)", "OpenSubtitles v3", { lang: "pob", nativeName: "Português (Brasil)" }),
  track("de", "German", "OpenSubtitles v3", { lang: "ger", nativeName: "Deutsch" }),
];
const names: Record<string, string> = { eng: "English", fre: "French", tur: "Turkish" };
const nameOf = (lang: string) => names[lang] ?? lang;

test("each run of one language is a section, indexes kept", () => {
  const sections = byLanguage([track("tr", "Turkish", "TOPAZ"), track("en", "English", "OpenSubtitles v3 1"), track("en", "English", "OpenSubtitles v3 2"), track("pt-BR", "Portuguese (Brazil)", "OpenSubtitles v3")]);
  expect(sections.map((s) => [s.name, s.tracks.map((t) => t.index)])).toEqual([
    ["Turkish", [0]],
    ["English", [1, 2]],
    ["Portuguese (Brazil)", [3]],
  ]);
});

test("the picker keeps the core's order, its own names only where they differ", () => {
  const langs = pickerLanguages(tracks, [], nameOf);
  expect(langs.map((l) => [l.language, l.name, l.own])).toEqual([
    ["tr", "Turkish", "Türkçe"],
    ["en", "English", null],
    ["pt-BR", "Portuguese (Brazil)", "Português (Brasil)"],
    ["de", "German", "Deutsch"],
  ]);
  expect(langs.map(optionCount)).toEqual([1, 2, 1, 1]);
  // The addon's code and the core's tag both count for a search.
  expect(langs[2]?.codes).toEqual(["pt-br", "pob"]);
});

test("a core with no own names (an older bridge) shows the English name alone", () => {
  const old = track("en", "English", "CAKES") as Partial<SubtitleTrack>;
  delete old.nativeName;
  expect(pickerLanguages([old as SubtitleTrack], [], nameOf)[0]?.own).toBeNull();
});

test("the file's tracks open their language first and win its one-tap pick", () => {
  const file = [
    { id: "3", lang: "fre" },
    { id: "2", lang: "eng" },
    { id: "9", lang: null },
  ];
  const langs = pickerLanguages(tracks, file, nameOf);
  // English gains the file's track; French, only in the file, comes after the core's languages.
  expect(langs.map((l) => l.name)).toEqual(["Turkish", "English", "Portuguese (Brazil)", "German", "French"]);
  const en = langs[1]!;
  expect(optionCount(en)).toBe(3);
  expect(bestOption(en)).toEqual({ file: { id: "2", lang: "eng" } });
  // No file track: the core's first, its best match.
  expect(bestOption(langs[0]!)).toEqual({ track: tracks[0]! });
  expect(bestOption({ ...en, file: [], tracks: [] })).toBeNull();
});

test("search matches the English name, the own name and the code", () => {
  const langs = pickerLanguages(tracks, [], nameOf);
  const find = (q: string) => langs.filter((l) => matchesLanguage(l, q)).map((l) => l.language);
  expect(find("")).toEqual(["tr", "en", "pt-BR", "de"]);
  expect(find("  ")).toEqual(["tr", "en", "pt-BR", "de"]);
  expect(find("Germ")).toEqual(["de"]);
  expect(find("deutsch")).toEqual(["de"]);
  // Accents and case do not matter, and a word inside the name counts.
  expect(find("turkce")).toEqual(["tr"]);
  expect(find("brazil")).toEqual(["pt-BR"]);
  expect(find("BRASIL")).toEqual(["pt-BR"]);
  expect(find("pob")).toEqual(["pt-BR"]);
  expect(find("ger")).toEqual(["de"]);
  expect(find("en")).toEqual(["en"]);
  // Only a word's start: "man" is inside "German", not at a word's start.
  expect(find("man")).toEqual([]);
  expect(find("xx")).toEqual([]);
});

test("the Subtitles button counts the languages the picker shows", () => {
  expect(offSummary(35, true)).toEqual({ text: "Off, 35 languages", searching: true });
  expect(offSummary(35, false)).toEqual({ text: "Off, 35 languages", searching: false });
  expect(offSummary(1, false)).toEqual({ text: "Off, 1 language", searching: false });
  expect(offSummary(0, true)).toEqual({ text: "Searching", searching: false });
  expect(offSummary(0, false)).toEqual({ text: "None found", searching: false });
});

test("the detail line: the source when the name does not say it, the kind, the match, the release", () => {
  expect(trackDetail(track("tr", "Turkish", "TOPAZ", { hearingImpaired: true }))).toEqual(["OpenSubtitles v3", "For the hard of hearing"]);
  expect(trackDetail(track("en", "English", "OpenSubtitles v3 2"))).toEqual([]);
  expect(trackDetail(track("en", "English", "OpenSubtitles v3", { forced: true }))).toEqual(["Forced"]);
  const release = "Slow.Horses.S06E02.1080p.WEB.H264-CAKES";
  // The rest of the line leaves it under 24: it takes a line of its own, cut only past the whole measure.
  expect(trackDetail(track("en", "English", "CAKES, WEB 1080p", { fits: true, release }))).toEqual(["OpenSubtitles v3", FITS, release]);
  expect(trackDetail(track("en", "English", "CAKES, WEB 1080p", { fits: true, release }), 34)).toEqual(["OpenSubtitles v3", FITS, middle(release, 34)]);
  expect(middle(release, 34)).toBe("Slow.Horses…1080p.WEB.H264-CAKES");
  // The release that is the name itself is said once.
  expect(trackDetail(track("en", "English", release, { release }))).toEqual(["OpenSubtitles v3"]);
});

test("a long release is cut in the middle at a dot, its group kept", () => {
  const raw = "Slow.Horses.S06E02.Daddy.Issues.1080p.ATVP.WEB-DL.DDP5.1.Atmos.H.264-FLUX";
  const cut = middle(raw, 46);
  expect(cut.length).toBeLessThanOrEqual(46);
  expect(cut.startsWith("Slow.Horses")).toBe(true);
  expect(cut.endsWith("-FLUX")).toBe(true);
  expect(cut).toContain("…");
  expect(middle("short.name", 46)).toBe("short.name");
  // The line's room: the rest of the line leaves the release at least 24, else it takes a line of its own.
  const parts = trackDetail(track("en", "English", "FLUX, WEB-DL 1080p", { fits: true, hearingImpaired: true, release: raw }), 46);
  expect(parts.at(-1)?.length).toBeLessThanOrEqual(46);
  expect(parts.at(-1)?.length).toBeGreaterThan(24);
});

test("a flag is the tag's own file, else its language's, else none", () => {
  const set = new Set(["en", "pt", "pt-br", "zh", "es"]);
  const has = (n: string) => set.has(n);
  expect(flagFile("pt-BR", has)).toBe("pt-br");
  expect(flagFile("pt", has)).toBe("pt");
  expect(flagFile("zh-Hant", has)).toBe("zh");
  expect(flagFile("es-419", has)).toBe("es");
  expect(flagFile("en", has)).toBe("en");
  expect(flagFile("mne", has)).toBeNull();
});
