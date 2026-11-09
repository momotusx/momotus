import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { barWords, chipCap, desktopFit, MIN_SCRUBBER, nextShown, phoneFit, positionStep, positionText, shiftName, shiftSaid, shiftShown, stripLanguages } from "./bar";

const track = (url: string, language = "en") => ({ url, lang: language, language, languageName: language }) as unknown as SubtitleTrack;

test("D4: the shift shows only while a listed core track is on, never with Off or the file's own track", () => {
  const tracks = [track("http://core/1"), track("http://core/2", "de")];
  expect(shiftShown("http://core/2", tracks)).toBe(true);
  // Off.
  expect(shiftShown(null, tracks)).toBe(false);
  expect(shiftShown(undefined, tracks)).toBe(false);
  // A track that left the list (the core could not read it) or none from the core (a file track plays).
  expect(shiftShown("http://core/9", tracks)).toBe(false);
  expect(shiftShown("http://core/1", [])).toBe(false);
});

test("D14: Next only when the core names the next episode", () => {
  expect(nextShown(null)).toBe(false);
  expect(nextShown(undefined)).toBe(false);
  expect(nextShown({ videoId: "tt1:1:2", title: "Two", season: 1, episode: 2, thumbnail: null })).toBe(true);
});

test("D5: Off aside, the language on, then the profile's with tracks, then the core's order, capped", () => {
  const langs = ["en", "tr", "de", "es", "fr", "it", "pt"].map((language) => ({ language }));
  const order = (on: string | null, profile: string[], cap = 5) => stripLanguages(langs, on, profile, cap).map((l) => l.language);
  expect(order(null, [])).toEqual(["en", "tr", "de", "es", "fr"]);
  // The profile names languages as addons do ("ger"); one with no tracks here ("ja") is skipped.
  expect(order(null, ["ger", "ja", "por"])).toEqual(["de", "pt", "en", "tr", "es"]);
  // The one on now leads, once.
  expect(order("it", ["ger", "ita"])).toEqual(["it", "de", "en", "tr", "es"]);
  expect(order("it", ["ger"], chipCap("phone"))).toEqual(["it", "de", "en"]);
  expect(chipCap("desktop")).toBe(5);
  expect(chipCap("tv")).toBe(5);
  // Fewer than the cap: all of them.
  expect(stripLanguages([{ language: "en" }], null, [], 5)).toHaveLength(1);
});

test("D16: words while a side fits, then icons per side, then the shift's chevrons, then two rows", () => {
  const left = { words: 360, icons: 164 };
  const right = { words: 680, icons: 474, chevrons: 410 };
  // A wide window: everything with words.
  expect(desktopFit(700, left, right)).toEqual({ leftWords: true, rightWords: true, chevrons: false, rows: 1 });
  // 1366 px: the right side drops its words, the left keeps them.
  expect(desktopFit(529, left, right)).toEqual({ leftWords: true, rightWords: false, chevrons: false, rows: 1 });
  // Narrower: the shift's words turn to chevrons.
  expect(desktopFit(440, left, right)).toEqual({ leftWords: true, rightWords: false, chevrons: true, rows: 1 });
  // 1024 and 721 px with a series: two rows; the left side drops its words where it overflows too.
  expect(desktopFit(366, left, right)).toEqual({ leftWords: true, rightWords: false, chevrons: true, rows: 2 });
  expect(desktopFit(214, left, right)).toEqual({ leftWords: false, rightWords: false, chevrons: true, rows: 2 });
  // Only the left overflowing its icons also takes two rows.
  expect(desktopFit(150, left, { words: 100, icons: 100, chevrons: 100 }).rows).toBe(2);
});

test("D16: the row depends on the groups' reserved widths, not on whether the shift shows now", () => {
  // The caller measures with the shift's room and the longest values always counted, so the same widths come in
  // with the subtitle on or off: one answer.
  const fit = (column: number) => desktopFit(column, { words: 360, icons: 164 }, { words: 680, icons: 474, chevrons: 410 });
  for (const column of [214, 366, 440, 529, 700]) expect(fit(column)).toEqual(fit(column));
});

test("D17: a phone sideways keeps one row while the scrubber has 120 px, chevrons first, then two rows", () => {
  const right = { words: 400, chevrons: 330 };
  // 844 px: 844 - 120 (times) - 156 - 400 = 168 for the scrubber.
  expect(phoneFit(844, 120, 156, right)).toEqual({ leftWords: false, rightWords: false, chevrons: false, rows: 1 });
  // 760 px: words leave 84, chevrons 154.
  expect(phoneFit(760, 120, 156, right)).toEqual({ leftWords: false, rightWords: false, chevrons: true, rows: 1 });
  // 640 px: under 120 either way, two rows, where the words fit the controls' own row.
  expect(phoneFit(640, 120, 156, right)).toEqual({ leftWords: false, rightWords: false, chevrons: false, rows: 2 });
  // A row too narrow for the words even alone keeps the chevrons.
  expect(phoneFit(520, 120, 156, right).chevrons).toBe(true);
  expect(MIN_SCRUBBER).toBe(120);
});

test("D7: Position steps 5 to 25, 8 reads Default", () => {
  expect(positionText(8)).toBe("Default");
  expect(positionText(12)).toBe("12%");
  expect(positionStep(8, 1)).toBe(12);
  expect(positionStep(8, -1)).toBe(5);
  expect(positionStep(5, -1)).toBeNull();
  expect(positionStep(25, 1)).toBeNull();
  // A synced value off the steps goes to the nearest one that way.
  expect(positionStep(10, 1)).toBe(12);
  expect(positionStep(10, -1)).toBe(8);
});

test("the shift's spoken line and its value's name", () => {
  expect(shiftSaid(0.25)).toBe("Subtitles 0.25 seconds later");
  expect(shiftSaid(-1.5)).toBe("Subtitles 1.5 seconds earlier");
  expect(shiftSaid(0)).toBe("Subtitles in sync");
  expect(shiftName(0.25, false)).toBe("+0.25 s, subtitles later, press to reset");
  expect(shiftName(-0.5, false)).toBe("−0.5 s, subtitles earlier, press to reset");
  expect(shiftName(0, false)).toBe("0.0 s, subtitles in sync");
  expect(shiftName(0.25, true)).toBe("+0.25 s, subtitles later, press OK to adjust");
  expect(shiftName(0, true)).toBe("0.0 s, subtitles in sync, press OK to adjust");
});

test("the strips take Off, Still searching and Choose a version from the shared words, as Android and Apple do", () => {
  expect([barWords.off, barWords.stillSearching, barWords.chooseVersion]).toEqual(["Off", "Still searching", "Choose a version"]);
  const strip = readFileSync(join(import.meta.dir, "../player/bar-strip.tsx"), "utf8");
  expect(strip).not.toMatch(/>\s*(Off|Still searching)\s*</);
  expect(strip).toContain("{barWords.off}");
  expect(strip).toContain("{barWords.stillSearching}");
  const player = readFileSync(join(import.meta.dir, "../screens/player.tsx"), "utf8");
  expect(player).toMatch(/data-f="audio:versions">\s*<span class="cl">\{barWords\.chooseVersion\}<\/span>/);
});
