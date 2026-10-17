import { expect, test } from "bun:test";
import { bytes, clock, parseVideo, releaseParts, runtimeMinutes, titleOfVideo } from "./format";

test("bytes", () => {
  expect(bytes(8.4e9)).toBe("8.4 GB");
  expect(bytes(612e6)).toBe("612 MB");
});

test("clock", () => {
  expect(clock(161)).toBe("2:41");
  expect(clock(3725)).toBe("1:02:05");
  expect(clock(-3)).toBe("0:00");
});

test("video ids", () => {
  expect(parseVideo("tt0944947:2:3")).toEqual({ id: "tt0944947", season: 2, episode: 3 });
  expect(parseVideo("tt0111161")).toBeNull();
  expect(titleOfVideo("tt0944947:2:3")).toBe("/title/series/tt0944947");
  expect(titleOfVideo("tt0111161")).toBe("/title/movie/tt0111161");
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
