import { expect, test } from "bun:test";
import { bytes, clock, episodeLabel, groupFailures, parseVideo, playPath, releaseParts, runtimeMinutes } from "./format";

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
