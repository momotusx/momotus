import { expect, test } from "bun:test";
import { airDate, bytes, clock, startsText, episodeLabel, groupFailures, initials, offersLibrary, parseVideo, playPath, posterLabel, ratingParts, releaseParts, runtimeMinutes } from "./format";

test("Add to Library shows only when the core says the Library can be set", () => {
  expect(offersLibrary({ librarySettable: true })).toBe(true);
  // A kids profile: no dead button (C44b review P2).
  expect(offersLibrary({ librarySettable: false })).toBe(false);
  expect(offersLibrary(undefined)).toBe(true);
});

test("bytes are 1024-based, as addons print sizes", () => {
  expect(bytes(5368709120)).toBe("5 GB");
  expect(bytes(2.2 * 1024 ** 3)).toBe("2.2 GB");
  expect(bytes(8.4e9)).toBe("7.8 GB");
  expect(bytes(150 * 1024 ** 3 + 1)).toBe("150 GB");
  expect(bytes(612 * 1024 ** 2)).toBe("612 MB");
});

test("clock", () => {
  expect(clock(161)).toBe("2:41");
  expect(clock(3725)).toBe("1:02:05");
  expect(clock(-3)).toBe("0:00");
});

test("video ids", () => {
  expect(parseVideo("tt0944947", "tt0944947:2:3")).toEqual({ season: 2, episode: 3 });
  expect(parseVideo("tt0111161", "tt0111161")).toBeNull();
  // Anime Kitsu: the title's id has a colon; what follows it is the episode.
  expect(parseVideo("kitsu:44042", "kitsu:44042:5")).toEqual({ season: null, episode: 5 });
  expect(episodeLabel(null, 5)).toBe("E5");
  expect(episodeLabel(2, 3)).toBe("S2:E3");
  expect(playPath("series", "tt0944947", "tt0944947:2:3")).toBe("/play/series/tt0944947/tt0944947:2:3");
});

test("runtime", () => {
  expect(runtimeMinutes("57 min")).toBe(57);
  expect(runtimeMinutes("1h 52min")).toBe(112);
  expect(runtimeMinutes(null)).toBeNull();
});

test("release names break between tokens only", () => {
  expect(releaseParts("Show.S02E03.2160p.DDP5.1.H.265-FLUX")).toEqual([
    "Show.",
    "S02E03.2160p.",
    "DDP5.1.",
    "H.265",
    "-FLUX",
  ]);
});

test("groupFailures says sources that failed alike once, in first order", () => {
  const gone = "The link no longer exists (404)";
  const slow = "The source did not answer in time";
  expect(
    groupFailures([
      { via: "Torrentio", error: slow },
      { via: "Torrentio", error: gone },
      { via: "Local", error: gone },
      { via: "Torrentio", error: gone },
    ]),
  ).toEqual([
    { via: "Torrentio", words: slow },
    { via: "Torrentio", words: `${gone} · 2 sources` },
    { via: "Local", words: gone },
  ]);
});

test("an air date reads as its UTC day", () => {
  expect(airDate("2026-10-27")).toBe("Oct 27, 2026");
  expect(airDate("2026-01-01T00:00:00Z")).toBe("Jan 1, 2026");
});

test("Up Next waits for the end, then counts the last 15 s", () => {
  expect(startsText(287)).toBe("Starts when this ends");
  expect(startsText(15.2)).toBe("Starts when this ends");
  expect(startsText(15)).toBe("Starts in 15 s");
  expect(startsText(11.3)).toBe("Starts in 12 s");
  expect(startsText(-0.5)).toBe("Starts in 0 s");
});

test("the IMDb score is plain text, and a missing score has no slot", () => {
  const text = (r: Parameters<typeof ratingParts>[0]) => ratingParts(r).map((p) => p.text);
  expect(text({ imdb: "8.8" })).toEqual(["IMDb 8.8"]);
  expect(text({ imdb: null })).toEqual([]);
  // A recording from before the field: nothing, never a placeholder.
  expect(text(undefined)).toEqual([]);
  // The ratings row sets the source's name apart from its score.
  expect(ratingParts({ imdb: "8.8" }).map((p) => [p.source, p.score])).toEqual([["IMDb", "8.8"]]);
  expect(ratingParts({ imdb: "10.0" }).map((p) => p.spoken)).toEqual(["IMDb 10.0"]);
  // A poster speaks the score the core sends.
  expect(posterLabel(["Inception", "2010"], { imdb: "8.8" })).toBe("Inception, 2010, IMDb 8.8");
  expect(posterLabel(["Inception", null], { imdb: null })).toBe("Inception");
  expect(posterLabel(["Inception"], null)).toBe("Inception");
});

test("initials stand in for a headshot", () => {
  expect(initials("Tim Robbins")).toBe("TR");
  expect(initials("édouard  Baer Hugo")).toBe("ÉB");
  expect(initials("Cher")).toBe("C");
  expect(initials("")).toBe("");
});
